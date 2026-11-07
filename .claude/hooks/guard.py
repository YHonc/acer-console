"""PreToolUse guard for Bash/PowerShell commands in this project.

Hardware, power, boot, disk, service, startup and security changes need the
user's per-item approval (AGENTS.md), so they return "ask". Deleting user
evidence (evidence/, research/) is refused outright. Recursive deletes elsewhere
ask and point at the 待删除-<date>/ convention; temp/build caches are exempt.

Test: echo '{"tool_name":"Bash","tool_input":{"command":"powercfg /setactive x"}}' | python -I .claude/hooks/guard.py
"""
import json
import re
import shlex
import sys

I = re.IGNORECASE

# Anywhere in the command text (covers PowerShell -Command strings and scripts piped inline).
ASK_ANYWHERE = [
    (r"--validate-", "acer-probe 硬件验证命令会写入设备"),
    (r"cargo\s+run\b[^;&|\n]*-p\s+acer-(probe|console)\b", "运行探针/桌面宿主会访问真实硬件"),
    (r"\b(npm\s+run\s+tauri|npx\s+tauri)\s+dev\b|\btauri\s+dev\b", "tauri dev 会启动带硬件后端的桌面应用"),
    (r"\b(Start|Stop|Restart|Set|New|Remove|Suspend)-Service\b", "改动 Windows 服务"),
    (r"\b(Invoke-CimMethod|Invoke-WmiMethod|Set-CimInstance|Set-WmiInstance|Remove-CimInstance)\b", "调用 WMI/CIM 方法可能写入硬件或系统"),
    (r"\b(Set|New|Remove)-ItemProperty\b[^;\n]*\\(Run|RunOnce)\b", "改动启动项 Run 键"),
    (r"\b(Restart|Stop)-Computer\b", "重启/关机"),
    (r"\bSet-ExecutionPolicy\b", "改动执行策略"),
    (r"\b(Format-Volume|Clear-Disk|Initialize-Disk|New-Partition|Set-Partition|Remove-Partition|Resize-Partition)\b", "磁盘/分区操作"),
    (r"\b(Enable|Disable)-WindowsOptionalFeature\b", "改动 Windows 功能"),
    (r"\bStart-Process\b[^;\n]*(acer-|DeviceCenter|device-center|PawnIO)", "启动本项目或驱动相关程序"),
    (r"PawnIO[^\s'\"]*\.exe|PawnIO_setup", "安装/运行 PawnIO 驱动"),
    (r"\bgit\s+(clean\s+-[a-z]*f|reset\s+--hard|push\b[^;\n]*--force|checkout\s+--\s+\.)", "不可逆的 git 操作"),
]

# First token of a command segment -> rule. A rule is either True (always ask)
# or a regex the remainder must match to ask.
ASK_PROGRAM = {
    "acer-helper": True,
    "acer-console": True,
    "acer-probe": True,
    "devicecenter": True,
    "device-center": True,
    "powercfg": r"^(?!\s*(/|-)(q|query|l|list|a|availablesleepstates|getactivescheme|energy|batteryreport|sleepstudy|srumutil|requests)\b)",
    "bcdedit": r"^(?!\s*(/|-)?(enum)\b|\s*$)",
    "bcdboot": True,
    "reg": r"^\s*(add|delete|import|restore|load|unload|copy)\b",
    "schtasks": r"/(create|delete|change|run|end)\b",
    "sc": r"^\s*(\\\\\S+\s+)?(stop|start|config|delete|create|pause|continue|failure|sdset)\b",
    "net": r"^\s*(stop|start|user|localgroup)\b",
    "diskpart": True,
    "mountvol": r"\S",
    "format": True,
    "manage-bde": r"-(on|off|lock|unlock|protectors)\b",
    "wmic": r"\b(call|set|delete|create)\b",
    "shutdown": True,
    "takeown": True,
    "msiexec": True,
}

PROTECTED = r"(?:^|[\s'\"=/\\])(?:\./)?(evidence|research)(?:[\\/]|['\"\s]|$)"
DELETE_CMD = r"^(rm|rmdir|del|erase|rd|Remove-Item|ri|git\s+rm|git\s+clean)\b"
SAFE_DELETE_TARGET = re.compile(
    r"(^|[\\/])(tmp|temp|target|node_modules|dist|__pycache__|\.svelte-kit)([\\/]|$)|AppData[\\/]Local[\\/]Temp|^/tmp|\$env:TEMP|%TEMP%|\.device-center-module-stage-",
    I,
)


def segments(cmd):
    for seg in re.split(r"&&|\|\||[;|\n]", cmd):
        seg = seg.strip().lstrip("(&").strip()
        while re.match(r"^\w+=\S*\s+", seg):  # leading VAR=value
            seg = re.sub(r"^\w+=\S*\s+", "", seg, count=1)
        if seg:
            yield seg


def program_of(seg):
    first = re.split(r"\s+", seg, maxsplit=1)
    name = first[0].strip("'\"").replace("\\", "/").rsplit("/", 1)[-1].lower()
    name = re.sub(r"\.(exe|cmd|bat|com)$", "", name)
    rest = first[1] if len(first) > 1 else ""
    return name, rest


def decide(cmd):
    for seg in segments(cmd):
        if re.match(DELETE_CMD, seg, I) and re.search(PROTECTED, seg, I):
            return "deny", "evidence/、research/ 是用户证据，禁止删除（AGENTS.md）。确需移动请先征得用户同意。"
    for pat, why in ASK_ANYWHERE:
        if re.search(pat, cmd, I):
            return "ask", f"需要用户逐项批准：{why}。"
    for seg in segments(cmd):
        name, rest = program_of(seg)
        rule = ASK_PROGRAM.get(name)
        if rule is True or (isinstance(rule, str) and re.search(rule, rest, I)):
            return "ask", f"需要用户逐项批准：{name} 会改动硬件、电源、引导、磁盘、服务或系统设置。"
        if re.match(DELETE_CMD, seg, I) and re.search(r"(^|\s)(-[a-z]*r[a-z]*|-Recurse|/s)\b", seg, I):
            try:
                args = [a for a in shlex.split(seg, posix=True)[1:] if not a.startswith("-") and not a.startswith("/")]
            except ValueError:
                args = seg.split()[1:]
            if not args or not all(SAFE_DELETE_TARGET.search(a) for a in args):
                return "ask", "递归删除：按约定应移到 待删除-<日期>/ 并写说明，由用户删除。确认这是临时产物再批准。"
    return None, None


def main():
    try:
        data = json.loads(sys.stdin.buffer.read().decode("utf-8"))
        cmd = (data.get("tool_input") or {}).get("command") or ""
        decision, reason = decide(cmd)
    except Exception as exc:  # fail towards asking, never silently allow on a parse error
        decision, reason = "ask", f"guard.py 解析失败（{exc.__class__.__name__}），请人工确认。"
    if decision:
        out = {"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": decision, "permissionDecisionReason": reason}}
        sys.stdout.write(json.dumps(out, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
