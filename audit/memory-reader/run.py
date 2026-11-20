"""Run the fixed two-thread SQLite fixture and ten opt-in Claude subscription calls."""

import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE.parent))
from claude_cli import EFFORT, MODEL, USAGE, model, require_login

ARMS = ("none", "raw", "structured")
WORLD = json.loads((HERE / "world.json").read_text())


def extract(cwd):
    transcript = []
    for n in range(1, WORLD["turns"] + 1):
        group = WORLD["threads"][n % 2]
        user = WORLD["messages"].get(str(n), WORLD["padding_user"])
        transcript.append(f'{n} {group} | {user} | {WORLD["assistant"]}')
    prompt = ("다음 합성 대화에서 나중에 조회할 구체적인 사람별 배정 사항을 담은 원문을 "
              "최대 12개 선택해 줘. 진행 중인 일상 활동과 단순 확인은 제외해. "
              "선택한 줄 번호만 JSON 정수 배열로 출력해.\n" + "\n".join(transcript))
    result, metrics = model(cwd, prompt, 0.5)
    chosen, json_format = parse_selection(result)
    return chosen, dict(metrics, json_format=json_format)


def parse_selection(result):
    """Accept one JSON array, optionally wrapped in a single Markdown code block."""
    result = result.strip()
    block = re.fullmatch(r"```(?:json)?[ \t]*\r?\n(.*?)\r?\n```", result, flags=re.DOTALL)
    try:
        chosen = json.loads(block.group(1) if block else result)
    except json.JSONDecodeError:
        raise RuntimeError("Extraction did not return one JSON array.") from None
    if (not isinstance(chosen, list) or len(chosen) > 12
            or any(type(n) != int or not 1 <= n <= WORLD["turns"] for n in chosen)
            or len(chosen) != len(set(chosen))):
        raise RuntimeError("Extraction returned invalid or duplicate turn numbers.")
    return chosen, "fenced_json" if block else "json"


def stored(chosen):
    args = ["cargo", "test", "--locked", "-q", "-p", "moosuri-store", "--lib",
            "export_namesake_reader_inputs", "--", "--ignored", "--nocapture"]
    proc = subprocess.run(args, cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True,
                          env={**os.environ, "MOOSURI_SYNTHETIC_TURN_NUMBERS": json.dumps(chosen)})
    line, = [v for v in proc.stdout.splitlines() if v.startswith("SYNTHETIC_READER=")]
    return json.loads(line.split("=", 1)[1])


def read(cwd, case, source):
    evidence = "\n".join(source)
    if len(evidence.encode("utf-8")) > 4096:
        raise ValueError("Reader evidence exceeded the common 4096-byte cap")
    prompt = ("근거만 보고 질문에 답해. 값 하나만 그대로 출력하고, 근거가 없으면 "
              "'확인 불가'만 출력해.\n질문: " + case["question"] + "\n근거:\n" + evidence)
    answer, metrics = model(cwd, prompt, 0.1)
    return dict(metrics, correct=answer == case["answer"], abstained=answer == "확인 불가")


def main():
    with tempfile.TemporaryDirectory(prefix="synthetic-reader-") as cwd:
        require_login(cwd)
        selected, extraction = extract(cwd)
        inputs = stored(selected)
        report = dict(model=MODEL, effort=EFFORT, history_turns=WORLD["turns"],
                      extraction=dict(extraction, selected=selected, write_us=inputs["write_us"]),
                      rows=[], aggregate={})
        for i, (case, row) in enumerate(zip(WORLD["cases"], inputs["rows"], strict=True)):
            assert case["id"] == row["id"]
            history = dict(id=case["id"], gold_rank=row["rank"], raw_us=row["raw_us"],
                           structured_us=row["structured_us"], outcomes={})
            for arm in ARMS[i:] + ARMS[:i]:
                history["outcomes"][arm] = read(cwd, case, row.get(arm, []))
            report["rows"].append(history)
        for arm in ARMS:
            rows = [r["outcomes"][arm] for r in report["rows"]]
            times = sorted(r["wall_ms"] + (history.get(arm + "_us", 0) / 1000)
                           for r, history in zip(rows, report["rows"], strict=True))
            report["aggregate"][arm] = {
                **{k: sum(r[k] for r in rows) for k in USAGE},
                "correct": sum(r["correct"] for r in rows),
                "n": len(rows), "p95_query_ms": round(times[math.ceil(0.95 * len(times)) - 1], 2),
            }
    print(json.dumps(report, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        raise SystemExit(str(error)) from None
