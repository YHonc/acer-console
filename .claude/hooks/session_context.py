"""SessionStart hook: inject a short project status so a new or resumed session
does not have to dig through old transcripts.

Prints JSON with additionalContext (ASCII-escaped, so console code pages don't matter).
Run by hand to preview: python -I .claude/hooks/session_context.py --plain
"""
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def git(*args):
    try:
        r = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, timeout=10)
        return r.stdout.decode("utf-8", "replace").strip()
    except Exception:
        return ""


def section(text, title, max_lines):
    m = re.search(rf"^## {re.escape(title)}[^\n]*\n(.*?)(?=^## |\Z)", text, re.S | re.M)
    if not m:
        return ""
    lines = [l for l in m.group(1).strip().splitlines() if l.strip()]
    more = len(lines) - max_lines
    return "\n".join(lines[:max_lines]) + (f"\n…（另有 {more} 行，见 HANDOFF.md）" if more > 0 else "")


def main():
    parts = []
    branch = git("branch", "--show-current")
    status = git("status", "--short").splitlines()
    parts.append(f"分支 {branch}；未提交改动 {len(status)} 项" + (":\n" + "\n".join(status[:12]) if status else "。"))
    parts.append("最近提交:\n" + git("log", "--oneline", "-5"))
    wts = [l for l in git("worktree", "list").splitlines()[1:]]
    if wts:
        parts.append("其他 worktree:\n" + "\n".join(wts))

    handoff = ROOT / "HANDOFF.md"
    if handoff.exists():
        t = handoff.read_text(encoding="utf-8")
        for title, n in (("当前目标", 12), ("下一步", 14)):
            s = section(t, title, n)
            if s:
                parts.append(f"HANDOFF「{title}」:\n{s}")

    runs = ROOT / "work/prompts/runs.md"
    if runs.exists():
        rows = [l for l in runs.read_text(encoding="utf-8").splitlines() if l.startswith("|") and not l.startswith("|---")]
        if len(rows) > 1:
            parts.append("Codex 委派最近记录（work/prompts/runs.md）:\n" + "\n".join(rows[-3:]))

    trash = sorted(p.name for p in ROOT.glob("待删除-*") if p.is_dir())
    if trash:
        parts.append("待用户删除的目录: " + "、".join(trash))

    text = "【项目状态，自动注入】完整状态见 docs/当前进度.md 与 HANDOFF.md。\n\n" + "\n\n".join(parts)
    if "--plain" in sys.argv:
        sys.stdout.reconfigure(encoding="utf-8")
        print(text)
    else:
        out = {"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}
        sys.stdout.write(json.dumps(out, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
