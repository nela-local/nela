// Sweeps llama-bench parameters across local GGUF models and reports best tok/s.
//
// Scope: benchmark-only tooling (does not modify the app). Linux-focused.
//
// Build:
//   gcc -O2 -std=c11 -Wall -Wextra -pedantic benchmark/llama_bench_sweep.c -o benchmark/llama_bench_sweep
//
// Run (from repo root):
//   ./benchmark/llama_bench_sweep
//   ./benchmark/llama_bench_sweep --help

#define _XOPEN_SOURCE 700

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <limits.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

// ----------------------------
// Small utilities
// ----------------------------

typedef struct {
    char **items;
    size_t len;
    size_t cap;
} StrVec;

static void vec_push(StrVec *v, const char *s) {
    if (v->len + 1 > v->cap) {
        size_t new_cap = v->cap ? v->cap * 2 : 16;
        char **new_items = (char **)realloc(v->items, new_cap * sizeof(char *));
        if (!new_items) {
            fprintf(stderr, "OOM\n");
            exit(1);
        }
        v->items = new_items;
        v->cap = new_cap;
    }
    v->items[v->len++] = strdup(s);
}

static void vec_free(StrVec *v) {
    for (size_t i = 0; i < v->len; i++) {
        free(v->items[i]);
    }
    free(v->items);
    v->items = NULL;
    v->len = 0;
    v->cap = 0;
}

static bool ends_with(const char *s, const char *suffix) {
    size_t sl = strlen(s);
    size_t su = strlen(suffix);
    if (su > sl) return false;
    return memcmp(s + (sl - su), suffix, su) == 0;
}

static char *path_join(const char *a, const char *b) {
    size_t al = strlen(a);
    size_t bl = strlen(b);
    bool need_slash = al > 0 && a[al - 1] != '/';
    size_t out_len = al + (need_slash ? 1 : 0) + bl + 1;
    char *out = (char *)malloc(out_len);
    if (!out) return NULL;
    snprintf(out, out_len, "%s%s%s", a, need_slash ? "/" : "", b);
    return out;
}

static bool is_dir(const char *path) {
    struct stat st;
    if (stat(path, &st) != 0) return false;
    return S_ISDIR(st.st_mode);
}

static bool is_file(const char *path) {
    struct stat st;
    if (stat(path, &st) != 0) return false;
    return S_ISREG(st.st_mode);
}

static void now_timestamp(char *buf, size_t buflen) {
    time_t t = time(NULL);
    struct tm tmv;
    localtime_r(&t, &tmv);
    strftime(buf, buflen, "%Y%m%d_%H%M%S", &tmv);
}

static void format_duration(double seconds, char *out, size_t out_len) {
    if (!out || out_len == 0) return;
    if (seconds < 0 || seconds > 1e12) {
        snprintf(out, out_len, "--:--:--");
        return;
    }

    long total = (long)(seconds + 0.5);
    long h = total / 3600;
    long m = (total % 3600) / 60;
    long s = total % 60;
    snprintf(out, out_len, "%02ld:%02ld:%02ld", h, m, s);
}

static void die(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fprintf(stderr, "\n");
    exit(1);
}

// ----------------------------
// Directory scan for *.gguf
// ----------------------------

static void scan_models_recursive(const char *dir_path, StrVec *out_models) {
    DIR *dir = opendir(dir_path);
    if (!dir) {
        fprintf(stderr, "warn: could not open dir '%s': %s\n", dir_path, strerror(errno));
        return;
    }

    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0) continue;
        char *child = path_join(dir_path, ent->d_name);
        if (!child) continue;

        if (is_dir(child)) {
            scan_models_recursive(child, out_models);
            free(child);
            continue;
        }

        if (is_file(child) && ends_with(child, ".gguf")) {
            vec_push(out_models, child);
        }

        free(child);
    }

    closedir(dir);
}

// ----------------------------
// Process execution + output capture
// ----------------------------

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} Buffer;

static void buf_init(Buffer *b) {
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static void buf_append(Buffer *b, const char *chunk, size_t n) {
    if (b->len + n + 1 > b->cap) {
        size_t new_cap = b->cap ? b->cap * 2 : 4096;
        while (new_cap < b->len + n + 1) new_cap *= 2;
        char *p = (char *)realloc(b->data, new_cap);
        if (!p) {
            fprintf(stderr, "OOM\n");
            exit(1);
        }
        b->data = p;
        b->cap = new_cap;
    }
    memcpy(b->data + b->len, chunk, n);
    b->len += n;
    b->data[b->len] = '\0';
}

static void buf_appendf(Buffer *b, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    va_list ap2;
    va_copy(ap2, ap);
    int needed = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    if (needed <= 0) {
        va_end(ap2);
        return;
    }
    size_t n = (size_t)needed;
    char *tmp = (char *)malloc(n + 1);
    if (!tmp) {
        va_end(ap2);
        fprintf(stderr, "OOM\n");
        exit(1);
    }
    vsnprintf(tmp, n + 1, fmt, ap2);
    va_end(ap2);
    buf_append(b, tmp, n);
    free(tmp);
}

static void json_append_escaped_string(Buffer *b, const char *s) {
    // Writes a JSON string literal with basic escaping.
    buf_append(b, "\"", 1);
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
        unsigned char c = *p;
        if (c == '"') {
            buf_append(b, "\\\"", 2);
        } else if (c == '\\') {
            buf_append(b, "\\\\", 2);
        } else if (c == '\n') {
            buf_append(b, "\\n", 2);
        } else if (c == '\r') {
            buf_append(b, "\\r", 2);
        } else if (c == '\t') {
            buf_append(b, "\\t", 2);
        } else if (c < 0x20) {
            // Control chars as \u00XX
            buf_appendf(b, "\\u%04x", (unsigned int)c);
        } else {
            buf_append(b, (const char *)p, 1);
        }
    }
    buf_append(b, "\"", 1);
}

static void buf_free(Buffer *b) {
    free(b->data);
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static int run_capture(const char *cwd, char *const argv[], Buffer *out) {
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        return -1;
    }

    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return -1;
    }

    if (pid == 0) {
        // Child
        if (cwd && chdir(cwd) != 0) {
            _exit(127);
        }
        dup2(pipefd[1], STDOUT_FILENO);
        dup2(pipefd[1], STDERR_FILENO);
        close(pipefd[0]);
        close(pipefd[1]);
        execv(argv[0], argv);
        _exit(127);
    }

    // Parent
    close(pipefd[1]);
    char buf[8192];
    ssize_t n;
    while ((n = read(pipefd[0], buf, sizeof(buf))) > 0) {
        buf_append(out, buf, (size_t)n);
    }
    close(pipefd[0]);

    int status = 0;
    waitpid(pid, &status, 0);
    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }
    return -1;
}

static void extract_first_error_line(const char *text, char *out, size_t out_len) {
    if (!out || out_len == 0) return;
    out[0] = '\0';
    if (!text) return;

    const char *candidates[] = {"main: error:", "error:", "ERROR:"};
    const char *p = NULL;
    for (size_t i = 0; i < sizeof(candidates) / sizeof(candidates[0]); i++) {
        const char *q = strstr(text, candidates[i]);
        if (q) {
            p = q;
            break;
        }
    }
    if (!p) {
        // Fallback: first non-empty line
        p = text;
        while (*p) {
            while (*p == '\n' || *p == '\r') p++;
            if (!*p) return;
            const char *e = strpbrk(p, "\r\n");
            size_t n = e ? (size_t)(e - p) : strlen(p);
            if (n > 0) {
                if (n >= out_len) n = out_len - 1;
                memcpy(out, p, n);
                out[n] = '\0';
                return;
            }
            p = e ? e : p + n;
        }
        return;
    }

    const char *e = strpbrk(p, "\r\n");
    size_t n = e ? (size_t)(e - p) : strlen(p);
    if (n >= out_len) n = out_len - 1;
    memcpy(out, p, n);
    out[n] = '\0';
}

// ----------------------------
// Parsing llama-bench jsonl lines
// ----------------------------

typedef struct {
    int n_prompt;
    int n_gen;
    double avg_ts;
} BenchLine;

static bool parse_int_field(const char *line, const char *key, int *out) {
    const char *p = strstr(line, key);
    if (!p) return false;
    p = strchr(p, ':');
    if (!p) return false;
    p++;
    while (*p && isspace((unsigned char)*p)) p++;
    // Expect integer
    char *end = NULL;
    long v = strtol(p, &end, 10);
    if (end == p) return false;
    *out = (int)v;
    return true;
}

static bool parse_double_field(const char *line, const char *key, double *out) {
    const char *p = strstr(line, key);
    if (!p) return false;
    p = strchr(p, ':');
    if (!p) return false;
    p++;
    while (*p && isspace((unsigned char)*p)) p++;
    char *end = NULL;
    double v = strtod(p, &end);
    if (end == p) return false;
    *out = v;
    return true;
}

static bool parse_bench_line(const char *line, BenchLine *out) {
    // We only need n_prompt, n_gen, avg_ts.
    int np = 0;
    int ng = 0;
    double ts = 0.0;
    if (!parse_int_field(line, "\"n_prompt\"", &np)) return false;
    if (!parse_int_field(line, "\"n_gen\"", &ng)) return false;
    if (!parse_double_field(line, "\"avg_ts\"", &ts)) return false;
    out->n_prompt = np;
    out->n_gen = ng;
    out->avg_ts = ts;
    return true;
}

// Splits output into lines; returns number of lines stored.
static size_t split_lines(char *text, char **lines, size_t max_lines) {
    size_t count = 0;
    char *p = text;
    while (p && *p && count < max_lines) {
        char *eol = strchr(p, '\n');
        if (eol) {
            *eol = '\0';
        }
        lines[count++] = p;
        if (!eol) break;
        p = eol + 1;
    }
    return count;
}

// ----------------------------
// Sweep logic
// ----------------------------

typedef struct {
    int threads;
    int n_prompt;
    int n_gen;
    int flash_attn;
    const char *ctk;
    const char *ctv;
    int batch;
    int ubatch;
    int reps;
    double pp_ts;
    double tg_ts;
} ComboResult;

static void print_usage(const char *prog) {
    printf("Usage: %s [options]\n\n", prog);
    printf("Runs llama-bench across local GGUF models and parameter grids, and reports best tok/s.\n\n");
    printf("Options:\n");
    printf("  --repo-root PATH         Repo root (default: .)\n");
    printf("  --llama-bench PATH       Path to llama-bench (default: genhat-desktop/src-tauri/bin/llama-lin/llama-bench)\n");
    printf("  --models-dir PATH        Directory to scan for .gguf (default: models/LLM)\n");
    printf("  --model PATH             Add a specific .gguf model to run (repeatable; bypasses --models-dir scan)\n");
    printf("  --out-dir PATH           Output directory (default: benchmark/results/llama_bench_sweep_<ts>)\n");
    printf("  --threads LIST           Comma list (default: 1,2,4,8)\n");
    printf("  --prompt LIST            n_prompt list (default: 512)\n");
    printf("  --gen LIST               n_gen list (default: 128)\n");
    printf("  --flash-attn LIST         0/1 list (default: 0,1)\n");
    printf("  --cache LIST             cache types for K and V (default: f16,q8_0)\n");
    printf("  --cache-k LIST           cache types for K only (default: f16,q8_0)\n");
    printf("  --cache-v LIST           cache types for V only (default: f16,q8_0)\n");
    printf("  --batch N                batch size (legacy single-value shortcut)\n");
    printf("  --ubatch N               ubatch size (legacy single-value shortcut)\n");
    printf("  --reps N                 repetitions per test (legacy single-value shortcut)\n");
    printf("  --batch-list LIST        batch size list (default: 2048)\n");
    printf("  --ubatch-list LIST       ubatch size list (default: 512)\n");
    printf("  --reps-list LIST         repetitions list (default: 1)\n");
    printf("  --limit N                limit number of models scanned (default: 0 = no limit)\n");
    printf("  --quick                  force compact defaults even with --model\n");
    printf("  --help                   Show this help\n");
}

static void parse_csv_ints(const char *s, int **out_arr, size_t *out_len) {
    *out_arr = NULL;
    *out_len = 0;
    if (!s || !*s) return;

    // Count commas.
    size_t count = 1;
    for (const char *p = s; *p; p++) if (*p == ',') count++;

    int *arr = (int *)calloc(count, sizeof(int));
    if (!arr) die("OOM");

    size_t idx = 0;
    const char *p = s;
    while (*p) {
        char *end = NULL;
        long v = strtol(p, &end, 10);
        if (end == p) break;
        arr[idx++] = (int)v;
        p = end;
        while (*p == ',' || isspace((unsigned char)*p)) p++;
    }

    *out_arr = arr;
    *out_len = idx;
}

static void parse_csv_strs(const char *s, StrVec *out) {
    out->items = NULL;
    out->len = 0;
    out->cap = 0;
    if (!s || !*s) return;

    const char *p = s;
    while (*p) {
        while (*p == ',' || isspace((unsigned char)*p)) p++;
        if (!*p) break;
        const char *start = p;
        while (*p && *p != ',') p++;
        size_t n = (size_t)(p - start);
        char tmp[128];
        if (n >= sizeof(tmp)) n = sizeof(tmp) - 1;
        memcpy(tmp, start, n);
        tmp[n] = '\0';
        // trim trailing spaces
        while (n > 0 && isspace((unsigned char)tmp[n - 1])) {
            tmp[n - 1] = '\0';
            n--;
        }
        vec_push(out, tmp);
        if (*p == ',') p++;
    }
}

static bool write_text_file(const char *path, const char *text) {
    FILE *f = fopen(path, "w");
    if (!f) return false;
    fputs(text, f);
    fclose(f);
    return true;
}

static void mkdir_p(const char *path) {
    char tmp[PATH_MAX];
    snprintf(tmp, sizeof(tmp), "%s", path);
    size_t len = strlen(tmp);
    if (len == 0) return;
    if (tmp[len - 1] == '/') tmp[len - 1] = '\0';

    for (char *p = tmp + 1; *p; p++) {
        if (*p == '/') {
            *p = '\0';
            mkdir(tmp, 0775);
            *p = '/';
        }
    }
    mkdir(tmp, 0775);
}

static void csv_write_header(FILE *f) {
    fprintf(
        f,
        "model_path,threads,n_prompt,n_gen,flash_attn,cache_type_k,cache_type_v,batch,ubatch,pp_tok_s,tg_tok_s\n");
}

static void csv_write_row(FILE *f, const char *model, const ComboResult *r) {
    fprintf(
        f,
        "\"%s\",%d,%d,%d,%d,%s,%s,%d,%d,%.6f,%.6f\n",
        model,
        r->threads,
        r->n_prompt,
        r->n_gen,
        r->flash_attn,
        r->ctk,
        r->ctv,
        r->batch,
        r->ubatch,
        r->pp_ts,
        r->tg_ts);
}

static bool run_one(
    const char *repo_root,
    const char *llama_bench,
    const char *model_path,
    int threads,
    int n_prompt,
    int n_gen,
    int flash_attn,
    const char *ctk,
    const char *ctv,
    int batch,
    int ubatch,
    int reps,
    ComboResult *out,
    char *err_msg,
    size_t err_msg_len) {

    // argv for execv. Use jsonl output so we can parse two lines (pp & tg).
    // We also use --no-warmup to reduce total time in sweeps.
    char threads_s[32], prompt_s[32], gen_s[32], fa_s[8], batch_s[32], ubatch_s[32], reps_s[32];
    snprintf(threads_s, sizeof(threads_s), "%d", threads);
    snprintf(prompt_s, sizeof(prompt_s), "%d", n_prompt);
    snprintf(gen_s, sizeof(gen_s), "%d", n_gen);
    snprintf(fa_s, sizeof(fa_s), "%d", flash_attn);
    snprintf(batch_s, sizeof(batch_s), "%d", batch);
    snprintf(ubatch_s, sizeof(ubatch_s), "%d", ubatch);
    snprintf(reps_s, sizeof(reps_s), "%d", reps);

    char *const argv[] = {
        (char *)llama_bench,
        (char *)"-m",
        (char *)model_path,
        (char *)"-t",
        threads_s,
        (char *)"-p",
        prompt_s,
        (char *)"-n",
        gen_s,
        (char *)"-fa",
        fa_s,
        (char *)"-ctk",
        (char *)ctk,
        (char *)"-ctv",
        (char *)ctv,
        (char *)"-b",
        batch_s,
        (char *)"-ub",
        ubatch_s,
        (char *)"-r",
        reps_s,
        (char *)"--no-warmup",
        (char *)"-o",
        (char *)"jsonl",
        NULL,
    };

    Buffer buf;
    buf_init(&buf);
    int code = run_capture(repo_root, argv, &buf);
    if (code != 0) {
        extract_first_error_line(buf.data, err_msg, err_msg_len);
        buf_free(&buf);
        return false;
    }

    // Parse up to 8 lines (usually 2). Also ignore loader noise lines if present.
    char *lines[32];
    size_t nlines = split_lines(buf.data, lines, 32);

    double pp_ts = -1.0;
    double tg_ts = -1.0;

    for (size_t i = 0; i < nlines; i++) {
        const char *ln = lines[i];
        // Skip non-JSON lines.
        const char *trim = ln;
        while (*trim && isspace((unsigned char)*trim)) trim++;
        if (*trim != '{') continue;

        BenchLine bl;
        if (!parse_bench_line(trim, &bl)) continue;
        if (bl.n_prompt > 0 && bl.n_gen == 0) {
            pp_ts = bl.avg_ts;
        } else if (bl.n_gen > 0) {
            tg_ts = bl.avg_ts;
        }
    }

    buf_free(&buf);

    if (pp_ts < 0 || tg_ts < 0) {
        if (err_msg && err_msg_len > 0) {
            snprintf(err_msg, err_msg_len, "Failed to parse jsonl output (pp/tg missing)");
        }
        return false;
    }

    out->threads = threads;
    out->n_prompt = n_prompt;
    out->n_gen = n_gen;
    out->flash_attn = flash_attn;
    out->ctk = ctk;
    out->ctv = ctv;
    out->batch = batch;
    out->ubatch = ubatch;
    out->reps = reps;
    out->pp_ts = pp_ts;
    out->tg_ts = tg_ts;
    if (err_msg && err_msg_len > 0) {
        err_msg[0] = '\0';
    }
    return true;
}

int main(int argc, char **argv) {
    const char *repo_root = ".";
    const char *llama_bench_rel = "genhat-desktop/src-tauri/bin/llama-lin/llama-bench";
    const char *models_dir_rel = "models/LLM";
    const char *out_dir = NULL;

    StrVec explicit_models = {0};

    const char *threads_csv = "1,2,4,8";
    const char *prompt_csv = "512";
    const char *gen_csv = "128";
    const char *flash_csv = "0,1";
    const char *cache_csv = NULL;
    const char *cache_k_csv = "f16,q8_0";
    const char *cache_v_csv = "f16,q8_0";
    const char *batch_csv = "2048";
    const char *ubatch_csv = "512";
    const char *reps_csv = "1";

    int limit_models = 0;
    bool quick_mode = false;

    bool user_set_threads = false;
    bool user_set_prompt = false;
    bool user_set_gen = false;
    bool user_set_flash = false;
    bool user_set_cache = false;
    bool user_set_cache_k = false;
    bool user_set_cache_v = false;
    bool user_set_batch = false;
    bool user_set_ubatch = false;
    bool user_set_reps = false;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            print_usage(argv[0]);
            return 0;
        } else if (strcmp(argv[i], "--repo-root") == 0 && i + 1 < argc) {
            repo_root = argv[++i];
        } else if (strcmp(argv[i], "--llama-bench") == 0 && i + 1 < argc) {
            llama_bench_rel = argv[++i];
        } else if (strcmp(argv[i], "--models-dir") == 0 && i + 1 < argc) {
            models_dir_rel = argv[++i];
        } else if (strcmp(argv[i], "--model") == 0 && i + 1 < argc) {
            vec_push(&explicit_models, argv[++i]);
        } else if (strcmp(argv[i], "--out-dir") == 0 && i + 1 < argc) {
            out_dir = argv[++i];
        } else if (strcmp(argv[i], "--threads") == 0 && i + 1 < argc) {
            threads_csv = argv[++i];
            user_set_threads = true;
        } else if (strcmp(argv[i], "--prompt") == 0 && i + 1 < argc) {
            prompt_csv = argv[++i];
            user_set_prompt = true;
        } else if (strcmp(argv[i], "--gen") == 0 && i + 1 < argc) {
            gen_csv = argv[++i];
            user_set_gen = true;
        } else if (strcmp(argv[i], "--flash-attn") == 0 && i + 1 < argc) {
            flash_csv = argv[++i];
            user_set_flash = true;
        } else if (strcmp(argv[i], "--cache") == 0 && i + 1 < argc) {
            cache_csv = argv[++i];
            cache_k_csv = cache_csv;
            cache_v_csv = cache_csv;
            user_set_cache = true;
            user_set_cache_k = true;
            user_set_cache_v = true;
        } else if (strcmp(argv[i], "--cache-k") == 0 && i + 1 < argc) {
            cache_k_csv = argv[++i];
            user_set_cache_k = true;
        } else if (strcmp(argv[i], "--cache-v") == 0 && i + 1 < argc) {
            cache_v_csv = argv[++i];
            user_set_cache_v = true;
        } else if (strcmp(argv[i], "--batch") == 0 && i + 1 < argc) {
            batch_csv = argv[++i];
            user_set_batch = true;
        } else if (strcmp(argv[i], "--ubatch") == 0 && i + 1 < argc) {
            ubatch_csv = argv[++i];
            user_set_ubatch = true;
        } else if (strcmp(argv[i], "--reps") == 0 && i + 1 < argc) {
            reps_csv = argv[++i];
            user_set_reps = true;
        } else if (strcmp(argv[i], "--batch-list") == 0 && i + 1 < argc) {
            batch_csv = argv[++i];
            user_set_batch = true;
        } else if (strcmp(argv[i], "--ubatch-list") == 0 && i + 1 < argc) {
            ubatch_csv = argv[++i];
            user_set_ubatch = true;
        } else if (strcmp(argv[i], "--reps-list") == 0 && i + 1 < argc) {
            reps_csv = argv[++i];
            user_set_reps = true;
        } else if (strcmp(argv[i], "--limit") == 0 && i + 1 < argc) {
            limit_models = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--quick") == 0) {
            quick_mode = true;
        } else {
            die("Unknown arg: %s (try --help)", argv[i]);
        }
    }

    // Default to a broad "complete sweep" profile for --model runs, unless user overrides fields.
    if (explicit_models.len > 0 && !quick_mode) {
        if (!user_set_threads) threads_csv = "1,2,4,6,8,12,16";
        if (!user_set_prompt) prompt_csv = "64,128,256,512,1024,2048,4096";
        if (!user_set_gen) gen_csv = "32,64,128,256,512";
        if (!user_set_flash) flash_csv = "0,1";
        if (!user_set_cache && !user_set_cache_k) cache_k_csv = "f16,q8_0,q4_0";
        if (!user_set_cache && !user_set_cache_v) cache_v_csv = "f16,q8_0,q4_0";
        if (!user_set_batch) batch_csv = "256,512,1024,2048,4096";
        if (!user_set_ubatch) ubatch_csv = "64,128,256,512,1024";
        if (!user_set_reps) reps_csv = "1";
    }

    char *llama_bench = NULL;
    char *models_dir = NULL;

    // Expand relative paths against repo_root if they are not absolute.
    if (llama_bench_rel[0] == '/') {
        llama_bench = strdup(llama_bench_rel);
    } else {
        llama_bench = path_join(repo_root, llama_bench_rel);
    }

    if (models_dir_rel[0] == '/') {
        models_dir = strdup(models_dir_rel);
    } else {
        models_dir = path_join(repo_root, models_dir_rel);
    }

    if (!llama_bench || !models_dir) die("OOM");

    if (!is_file(llama_bench)) {
        die("llama-bench not found at '%s' (use --llama-bench)", llama_bench);
    }

    if (!is_dir(models_dir)) {
        if (explicit_models.len == 0) {
            die("models dir not found at '%s' (use --models-dir or --model)", models_dir);
        }
    }

    // Parse sweep lists.
    int *threads = NULL, *prompts = NULL, *gens = NULL, *flashes = NULL;
    int *batches = NULL, *ubatches = NULL, *reps_list = NULL;
    size_t n_threads = 0, n_prompts = 0, n_gens = 0, n_flashes = 0;
    size_t n_batches = 0, n_ubatches = 0, n_reps_list = 0;

    parse_csv_ints(threads_csv, &threads, &n_threads);
    parse_csv_ints(prompt_csv, &prompts, &n_prompts);
    parse_csv_ints(gen_csv, &gens, &n_gens);
    parse_csv_ints(flash_csv, &flashes, &n_flashes);
    parse_csv_ints(batch_csv, &batches, &n_batches);
    parse_csv_ints(ubatch_csv, &ubatches, &n_ubatches);
    parse_csv_ints(reps_csv, &reps_list, &n_reps_list);

    if (
        n_threads == 0 || n_prompts == 0 || n_gens == 0 || n_flashes == 0 || n_batches == 0 || n_ubatches == 0 ||
        n_reps_list == 0) {
        die("Invalid sweep lists (threads/prompt/gen/flash-attn/batch/ubatch/reps)");
    }

    StrVec caches_k;
    StrVec caches_v;
    parse_csv_strs(cache_k_csv, &caches_k);
    parse_csv_strs(cache_v_csv, &caches_v);
    if (caches_k.len == 0) {
        die("Invalid --cache-k list");
    }
    if (caches_v.len == 0) {
        die("Invalid --cache-v list");
    }

    // Resolve model list.
    StrVec models = {0};
    if (explicit_models.len > 0) {
        for (size_t i = 0; i < explicit_models.len; i++) {
            const char *mp = explicit_models.items[i];
            char *resolved = NULL;
            if (mp[0] == '/') {
                resolved = strdup(mp);
            } else {
                resolved = path_join(repo_root, mp);
            }
            if (!resolved) die("OOM");
            if (!is_file(resolved) || !ends_with(resolved, ".gguf")) {
                die("--model must point to a .gguf file: '%s'", resolved);
            }
            vec_push(&models, resolved);
            free(resolved);
        }
    } else {
        scan_models_recursive(models_dir, &models);
        if (models.len == 0) {
            die("No .gguf models found under '%s'", models_dir);
        }
    }

    size_t total_models_to_run = models.len;
    if (limit_models > 0 && (size_t)limit_models < total_models_to_run) {
        total_models_to_run = (size_t)limit_models;
    }

    size_t combos_per_model =
        n_threads * n_prompts * n_gens * n_flashes * caches_k.len * caches_v.len * n_batches * n_ubatches * n_reps_list;
    size_t total_combos = combos_per_model * total_models_to_run;

    fprintf(
        stderr,
        "Sweep grid: %zu combinations/model across %zu model(s) => %zu total run(s).\n",
        combos_per_model,
        total_models_to_run,
        total_combos);
    if (combos_per_model > 2000) {
        fprintf(stderr, "note: large sweep detected; this may take a long time. Use --quick or narrower lists to reduce runtime.\n");
    }

    // Output dir
    char ts[32];
    now_timestamp(ts, sizeof(ts));

    char default_out[PATH_MAX];
    snprintf(default_out, sizeof(default_out), "benchmark/results/llama_bench_sweep_%s", ts);

    const char *out_dir_effective = out_dir ? out_dir : default_out;
    char *out_dir_abs = NULL;
    if (out_dir_effective[0] == '/') {
        out_dir_abs = strdup(out_dir_effective);
    } else {
        out_dir_abs = path_join(repo_root, out_dir_effective);
    }
    if (!out_dir_abs) die("OOM");

    mkdir_p(out_dir_abs);

    char results_csv_path[PATH_MAX];
    char best_csv_path[PATH_MAX];
    char best_pp_csv_path[PATH_MAX];
    char summary_json_path[PATH_MAX];
    snprintf(results_csv_path, sizeof(results_csv_path), "%s/all_results.csv", out_dir_abs);
    snprintf(best_csv_path, sizeof(best_csv_path), "%s/best_by_model.csv", out_dir_abs);
    snprintf(best_pp_csv_path, sizeof(best_pp_csv_path), "%s/best_by_model_pp.csv", out_dir_abs);
    snprintf(summary_json_path, sizeof(summary_json_path), "%s/summary.json", out_dir_abs);

    FILE *all_csv = fopen(results_csv_path, "w");
    if (!all_csv) die("Failed to open %s", results_csv_path);
    csv_write_header(all_csv);

    FILE *best_csv = fopen(best_csv_path, "w");
    if (!best_csv) die("Failed to open %s", best_csv_path);
    fprintf(best_csv, "model_path,best_metric,best_tg_tok_s,best_pp_tok_s,threads,n_prompt,n_gen,flash_attn,cache_type_k,cache_type_v,batch,ubatch\n");

    FILE *best_pp_csv = fopen(best_pp_csv_path, "w");
    if (!best_pp_csv) die("Failed to open %s", best_pp_csv_path);
    fprintf(best_pp_csv, "model_path,best_metric,best_tg_tok_s,best_pp_tok_s,threads,n_prompt,n_gen,flash_attn,cache_type_k,cache_type_v,batch,ubatch\n");

    // JSON summary: keep it simple to avoid full JSON writer complexity.
    Buffer json;
    buf_init(&json);
    buf_append(&json, "{\n  \"tool\": \"llama_bench_sweep\",\n", strlen("{\n  \"tool\": \"llama_bench_sweep\",\n"));
    {
        char tmp[256];
        snprintf(tmp, sizeof(tmp), "  \"timestamp\": \"%s\",\n", ts);
        buf_append(&json, tmp, strlen(tmp));
    }
    buf_append(&json, "  \"models\": [\n", strlen("  \"models\": [\n"));

    bool first_json_model = true;

    int scanned = 0;
    int total_successes = 0;
    size_t total_attempts = 0;
    time_t sweep_start = time(NULL);
    time_t last_progress_print = 0;

    for (size_t mi = 0; mi < models.len; mi++) {
        if (limit_models > 0 && scanned >= limit_models) break;
        scanned++;

        const char *model_path = models.items[mi];

        ComboResult best_tg = {0};
        ComboResult best_pp = {0};
        best_tg.pp_ts = -1.0;
        best_tg.tg_ts = -1.0;
        best_pp.pp_ts = -1.0;
        best_pp.tg_ts = -1.0;

        int model_attempts = 0;
        int model_successes = 0;
        char last_err[256];
        last_err[0] = '\0';

        fprintf(stderr, "[%d/%zu] Sweeping %s\n", scanned, total_models_to_run, model_path);

        for (size_t ti = 0; ti < n_threads; ti++) {
            for (size_t pi = 0; pi < n_prompts; pi++) {
                for (size_t gi = 0; gi < n_gens; gi++) {
                    for (size_t fi = 0; fi < n_flashes; fi++) {
                        for (size_t cki = 0; cki < caches_k.len; cki++) {
                            for (size_t cvi = 0; cvi < caches_v.len; cvi++) {
                                for (size_t bi = 0; bi < n_batches; bi++) {
                                    for (size_t ubi = 0; ubi < n_ubatches; ubi++) {
                                        for (size_t ri = 0; ri < n_reps_list; ri++) {
                                            const char *ctk = caches_k.items[cki];
                                            const char *ctv = caches_v.items[cvi];

                                            ComboResult r;
                                            memset(&r, 0, sizeof(r));

                                            model_attempts++;
                                            total_attempts++;

                                            time_t now = time(NULL);
                                            if (total_combos > 0 &&
                                                (total_attempts == 1 || total_attempts == total_combos ||
                                                 now - last_progress_print >= 1)) {
                                                double elapsed = difftime(now, sweep_start);
                                                double rate = elapsed > 0 ? ((double)total_attempts / elapsed) : 0.0;
                                                double eta_s = rate > 0.0 ? ((double)(total_combos - total_attempts) / rate) : -1.0;
                                                double pct = 100.0 * ((double)total_attempts / (double)total_combos);
                                                double model_pct =
                                                    combos_per_model > 0
                                                        ? 100.0 * ((double)model_attempts / (double)combos_per_model)
                                                        : 0.0;
                                                char eta_buf[32];
                                                format_duration(eta_s, eta_buf, sizeof(eta_buf));

                                                fprintf(
                                                    stderr,
                                                    "\rProgress %zu/%zu (%.2f%%) | Model %d/%zu: %d/%zu (%.2f%%) | Success %d | %.2f run/s | ETA %s",
                                                    total_attempts,
                                                    total_combos,
                                                    pct,
                                                    scanned,
                                                    total_models_to_run,
                                                    model_attempts,
                                                    combos_per_model,
                                                    model_pct,
                                                    total_successes,
                                                    rate,
                                                    eta_buf);
                                                fflush(stderr);
                                                last_progress_print = now;
                                            }

                                            bool ok = run_one(
                                                repo_root,
                                                llama_bench,
                                                model_path,
                                                threads[ti],
                                                prompts[pi],
                                                gens[gi],
                                                flashes[fi],
                                                ctk,
                                                ctv,
                                                batches[bi],
                                                ubatches[ubi],
                                                reps_list[ri],
                                                &r,
                                                last_err,
                                                sizeof(last_err));

                                            if (!ok) {
                                                continue;
                                            }

                                            model_successes++;
                                            total_successes++;

                                            csv_write_row(all_csv, model_path, &r);

                                            if (r.tg_ts > best_tg.tg_ts) {
                                                best_tg = r;
                                            }
                                            if (r.pp_ts > best_pp.pp_ts) {
                                                best_pp = r;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if (total_combos > 0) {
            fprintf(stderr, "\n");
        }

        // Emit best rows.
        if (model_successes == 0) {
            if (last_err[0]) {
                fprintf(
                    stderr,
                    "warn: no successful runs for %s (0/%d attempts). last error: %s\n",
                    model_path,
                    model_attempts,
                    last_err);
            } else {
                fprintf(stderr, "warn: no successful runs for %s (0/%d attempts)\n", model_path, model_attempts);
            }
        }

        if (best_tg.tg_ts > 0) {
            fprintf(
                best_csv,
                "\"%s\",tg,%.6f,%.6f,%d,%d,%d,%d,%s,%s,%d,%d\n",
                model_path,
                best_tg.tg_ts,
                best_tg.pp_ts,
                best_tg.threads,
                best_tg.n_prompt,
                best_tg.n_gen,
                best_tg.flash_attn,
                best_tg.ctk,
                best_tg.ctv,
                best_tg.batch,
                best_tg.ubatch);
        }

        if (best_pp.pp_ts > 0) {
            fprintf(
                best_pp_csv,
                "\"%s\",pp,%.6f,%.6f,%d,%d,%d,%d,%s,%s,%d,%d\n",
                model_path,
                best_pp.tg_ts,
                best_pp.pp_ts,
                best_pp.threads,
                best_pp.n_prompt,
                best_pp.n_gen,
                best_pp.flash_attn,
                best_pp.ctk,
                best_pp.ctv,
                best_pp.batch,
                best_pp.ubatch);
        }

        // JSON entry (only if we got at least one best).
        if (best_tg.tg_ts > 0 || best_pp.pp_ts > 0) {
            if (!first_json_model) {
                buf_append(&json, ",\n", 2);
            }
            first_json_model = false;

            buf_append(&json, "    {\n      \"model_path\": ", strlen("    {\n      \"model_path\": "));
            json_append_escaped_string(&json, model_path);
            buf_append(&json, ",\n      \"best_by_tg\": ", strlen(",\n      \"best_by_tg\": "));

            if (best_tg.tg_ts > 0) {
                buf_append(&json, "{\n", 2);
                buf_appendf(&json, "        \"threads\": %d,\n", best_tg.threads);
                buf_appendf(&json, "        \"n_prompt\": %d,\n", best_tg.n_prompt);
                buf_appendf(&json, "        \"n_gen\": %d,\n", best_tg.n_gen);
                buf_appendf(&json, "        \"flash_attn\": %d,\n", best_tg.flash_attn);
                buf_append(&json, "        \"cache_type_k\": ", strlen("        \"cache_type_k\": "));
                json_append_escaped_string(&json, best_tg.ctk);
                buf_append(&json, ",\n        \"cache_type_v\": ", strlen(",\n        \"cache_type_v\": "));
                json_append_escaped_string(&json, best_tg.ctv);
                buf_append(&json, ",\n", 2);
                buf_appendf(&json, "        \"batch\": %d,\n", best_tg.batch);
                buf_appendf(&json, "        \"ubatch\": %d,\n", best_tg.ubatch);
                buf_appendf(&json, "        \"pp_tok_s\": %.6f,\n", best_tg.pp_ts);
                buf_appendf(&json, "        \"tg_tok_s\": %.6f\n", best_tg.tg_ts);
                buf_append(&json, "      }", strlen("      }"));
            } else {
                buf_append(&json, "null", 4);
            }

            buf_append(&json, ",\n      \"best_by_pp\": ", strlen(",\n      \"best_by_pp\": "));

            if (best_pp.pp_ts > 0) {
                buf_append(&json, "{\n", 2);
                buf_appendf(&json, "        \"threads\": %d,\n", best_pp.threads);
                buf_appendf(&json, "        \"n_prompt\": %d,\n", best_pp.n_prompt);
                buf_appendf(&json, "        \"n_gen\": %d,\n", best_pp.n_gen);
                buf_appendf(&json, "        \"flash_attn\": %d,\n", best_pp.flash_attn);
                buf_append(&json, "        \"cache_type_k\": ", strlen("        \"cache_type_k\": "));
                json_append_escaped_string(&json, best_pp.ctk);
                buf_append(&json, ",\n        \"cache_type_v\": ", strlen(",\n        \"cache_type_v\": "));
                json_append_escaped_string(&json, best_pp.ctv);
                buf_append(&json, ",\n", 2);
                buf_appendf(&json, "        \"batch\": %d,\n", best_pp.batch);
                buf_appendf(&json, "        \"ubatch\": %d,\n", best_pp.ubatch);
                buf_appendf(&json, "        \"pp_tok_s\": %.6f,\n", best_pp.pp_ts);
                buf_appendf(&json, "        \"tg_tok_s\": %.6f\n", best_pp.tg_ts);
                buf_append(&json, "      }", strlen("      }"));
            } else {
                buf_append(&json, "null", 4);
            }

            buf_append(&json, "\n    }", strlen("\n    }"));
        }
    }

    buf_append(&json, "  ]\n}\n", strlen("  ]\n}\n"));

    fclose(all_csv);
    fclose(best_csv);
    fclose(best_pp_csv);

    if (!write_text_file(summary_json_path, json.data ? json.data : "{}")) {
        fprintf(stderr, "warn: failed to write %s\n", summary_json_path);
    }

    buf_free(&json);

    fprintf(stderr, "\nDone. Outputs:\n");
    fprintf(stderr, "  %s\n", results_csv_path);
    fprintf(stderr, "  %s\n", best_csv_path);
    fprintf(stderr, "  %s\n", best_pp_csv_path);
    fprintf(stderr, "  %s\n", summary_json_path);

    free(out_dir_abs);
    free(llama_bench);
    free(models_dir);
    free(threads);
    free(prompts);
    free(gens);
    free(flashes);
    free(batches);
    free(ubatches);
    free(reps_list);
    vec_free(&caches_k);
    vec_free(&caches_v);
    vec_free(&models);
    vec_free(&explicit_models);

    if (total_successes == 0) {
        fprintf(stderr, "\nerror: no successful llama-bench runs produced any results.\n");
        fprintf(stderr, "Hint: try smaller models, fewer threads, smaller --batch/--ubatch, or fewer cache combinations.\n");
        return 2;
    }

    return 0;
}
