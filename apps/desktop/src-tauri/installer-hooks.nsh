!macro NSIS_HOOK_PREUNINSTALL
  MessageBox MB_YESNO|MB_ICONQUESTION "Keep easyJob data in $INSTDIR\data? Choose No to delete it." IDYES easyjob_keep_data
  RMDir /r "$INSTDIR\data"
  easyjob_keep_data:
!macroend
