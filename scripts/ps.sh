#!/usr/bin/env bash
# Run a Windows PowerShell 5.1 script with UTF-8 console output, so bash sees
# readable Chinese instead of GBK bytes. Exit code is the script's.
#
#   scripts/ps.sh path/to/script.ps1 [-Param value ...]
#   scripts/ps.sh -c 'Get-Date; Write-Output "中文"'
#
# 5.1 reads a .ps1 without a BOM as the ANSI code page (GBK here), so any
# non-ASCII script must be saved as UTF-8 *with BOM*; this wrapper warns if not.
set -u
if [ $# -lt 1 ]; then sed -n 2,9p "$0"; exit 2; fi

PRE='[Console]::OutputEncoding=[Text.Encoding]::UTF8; $OutputEncoding=[Text.Encoding]::UTF8; $ErrorActionPreference="Stop";'

if [ "$1" = "-c" ]; then
  shift
  exec powershell -NoProfile -ExecutionPolicy Bypass -Command "$PRE $*"
fi

script=$1; shift
if [ ! -f "$script" ]; then echo "ps.sh: 找不到脚本 $script" >&2; exit 2; fi
if LC_ALL=C grep -q '[^[:print:][:space:]]' "$script" && [ "$(head -c3 "$script" | od -An -tx1 | tr -d ' ')" != "efbbbf" ]; then
  echo "ps.sh: 警告：$script 含非 ASCII 字符但没有 UTF-8 BOM，PowerShell 5.1 会按 GBK 解析，可能报语法错误。" >&2
fi

# Quote each argument for PowerShell (single quotes, '' escapes a quote).
args=""
for a in "$@"; do
  case "$a" in
    -[A-Za-z]*) args="$args $a" ;;                       # parameter names stay bare
    *) args="$args '$(printf '%s' "$a" | sed "s/'/''/g")'" ;;
  esac
done
win_script=$(cygpath -w "$script" 2>/dev/null || printf '%s' "$script")
exec powershell -NoProfile -ExecutionPolicy Bypass -Command \
  "$PRE & '$(printf '%s' "$win_script" | sed "s/'/''/g")'$args; \$ok=\$?; \$c=[int]\$LASTEXITCODE; if (\$c -ne 0) { exit \$c } elseif (-not \$ok) { exit 1 } else { exit 0 }"
