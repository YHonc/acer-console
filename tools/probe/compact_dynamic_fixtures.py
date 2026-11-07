"""Preserve the relevant real read/write fields without duplicating whole logs."""
from pathlib import Path
import json
root = Path(__file__).resolve().parents[2]
def snapshot(value):
    return {"captured_at_ms": value["captured_at_ms"], "reads": [read for read in value["reads"] if read["method"] in ("GetGamingKBBacklight", "GetGamingRgbKb")]}
result = []
for code in range(1, 8):
    source = root / "evidence" / f"lighting-effect-{code}-restore.json"
    data = json.loads(source.read_text(encoding="utf-8-sig"))
    result.append({"source": str(source.relative_to(root)), "code": code, "outcome": data["outcome"], "target_readback_confirmed": data["target_readback_confirmed"], "restore_readback_confirmed": data["restore_readback_confirmed"], "before": snapshot(data["trial_apply"]["before"]), "after": snapshot(data["target_snapshot"]), "writes": data["trial_apply"]["writes"], "restore_writes": data["restore_apply"]["writes"], "restored": snapshot(data["restored_snapshot"])})
target = root / "crates" / "acer-protocol" / "tests" / "fixtures" / "phn16-71-seven-dynamic-configs.json"
target.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
print(f"Wrote {len(result)} actual configurations; {target.stat().st_size} bytes")
