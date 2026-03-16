CC ?= gcc
CFLAGS ?= -O2 -std=c11 -Wall -Wextra -pedantic

.PHONY: all clean

all: llama_bench_sweep

llama_bench_sweep: llama_bench_sweep.c
	$(CC) $(CFLAGS) $< -o $@

clean:
	rm -f llama_bench_sweep
