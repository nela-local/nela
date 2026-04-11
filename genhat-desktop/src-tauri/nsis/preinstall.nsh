; GenHat NSIS installer hooks
; Called by Tauri's NSIS installer at pre-install time.
; Creates the models directory structure under $INSTDIR so that
; downloaded model files can be placed in the correct subfolders.
;
; Subdirectory layout matches models.toml model_file paths.

!macro NSIS_HOOK_PREINSTALL
    CreateDirectory "$INSTDIR\models"
    CreateDirectory "$INSTDIR\models\LLM"
    CreateDirectory "$INSTDIR\models\LiquidAI-VLM"
    CreateDirectory "$INSTDIR\models\bge-1.5-embed"
    CreateDirectory "$INSTDIR\models\distilBert-query-router"
    CreateDirectory "$INSTDIR\models\distilBert-query-router\onnx_model"
    CreateDirectory "$INSTDIR\models\tts"
    CreateDirectory "$INSTDIR\models\tts\kitten-tts"
    CreateDirectory "$INSTDIR\models\tts\kitten-tts\mini"
    CreateDirectory "$INSTDIR\models\grader"
    CreateDirectory "$INSTDIR\models\grader\ms-marco-MiniLM-L6-v2-onnx-int8"
    CreateDirectory "$INSTDIR\models\parakeet"
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
