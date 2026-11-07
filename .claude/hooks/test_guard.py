"""Table test for guard.py: python -I .claude/hooks/test_guard.py"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from guard import decide  # noqa: E402

CASES = [
    # (command, expected decision or None)
    ("cargo run -p acer-probe -- --validate-fan-max-restore", "ask"),
    ("target/release/acer-probe.exe --capabilities", "ask"),
    ("./artifacts/X/acer-helper.exe", "ask"),
    ("cd apps/desktop && npm run tauri dev", "ask"),
    ("powercfg /setactive SCHEME_MIN", "ask"),
    ("powercfg /q", None),
    ("powercfg -getactivescheme", None),
    ("bcdedit /set testsigning on", "ask"),
    ("bcdedit /enum", None),
    ("reg add HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v X /d y", "ask"),
    ("reg query HKLM\\SYSTEM", None),
    ("schtasks /create /tn x /tr y", "ask"),
    ("schtasks /query", None),
    ("sc stop AcerQAAgent", "ask"),
    ("sc query", None),
    ('powershell -NoProfile -Command "Stop-Service AcerSysMonitor"', "ask"),
    ('powershell -Command "Invoke-CimMethod -ClassName AcerGamingFunction"', "ask"),
    ('powershell -Command "Get-CimInstance Win32_Battery"', None),
    ("Start-Process .\\modules\\device-center\\DeviceCenter.exe", "ask"),
    ("tools/reverse/pawnio-2.2.0/PawnIO_setup.exe -install", "ask"),
    ("diskpart /s x.txt", "ask"),
    ("mountvol X: /s", "ask"),
    ("git reset --hard HEAD~1", "ask"),
    ("git clean -fdx", "ask"),
    ("rm -rf evidence/old", "deny"),
    ("git rm -r research/opencore", "deny"),
    ("Remove-Item -Recurse .\\evidence\\x", "deny"),
    ("rm docs/thermal-research-20261103/x.md", None),
    ("rm -rf work/U3", "ask"),
    ("rm -rf /tmp/x %USERPROFILE%/AppData/Local/Temp/dc-target", None),
    ("rm -rf target/release/build", None),
    ("rm -rf artifacts/X/modules/.device-center-module-stage-abc", None),
    ("cargo test --workspace --locked", None),
    ("cargo test -p acer-probe", None),
    ("npm run check && npm run build", None),
    ("git status --short; git log --oneline -3", None),
    ("ls artifacts/AcerConsole-DCIPC-20261107/acer-helper.exe", None),
    ("grep -n acer-console docs/README.md", None),
    ("python design/apple-redesign-20261105/check_app_dc.py http://127.0.0.1:4173/", None),
]

fails = 0
for cmd, want in CASES:
    got, why = decide(cmd)
    ok = got == want
    fails += not ok
    print(("PASS" if ok else "FAIL"), f"{str(got):5} want={str(want):5} {cmd}")
print(f"{len(CASES) - fails}/{len(CASES)} passed")
sys.exit(1 if fails else 0)
