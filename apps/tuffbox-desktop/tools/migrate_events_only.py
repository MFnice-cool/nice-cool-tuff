#!/usr/bin/env python3
"""Safe on: -> native event migration only."""
import re
from pathlib import Path

FILES = [
    "src/App.svelte",
    "src/components/BriefEditor.svelte",
    "src/components/CatalogProjectView.svelte",
    "src/components/ChangeHistory.svelte",
    "src/components/ConfigEditor.svelte",
    "src/components/CrashVotes.svelte",
    "src/components/Diagnostics.svelte",
    "src/components/ExportBuilder.svelte",
    "src/components/LaunchLogModal.svelte",
    "src/components/Settings.svelte",
]

EVENT_MAP = {
    "click": "onclick",
    "change": "onchange",
    "input": "oninput",
    "blur": "onblur",
    "keydown": "onkeydown",
    "keyup": "onkeyup",
    "submit": "onsubmit",
    "drop": "ondrop",
    "scroll": "onscroll",
    "ready": "onready",
}


def convert_events(content: str) -> str:
    # bare dragover preventDefault
    content = content.replace(
        "on:dragover|preventDefault",
        'ondragover={(e) => e.preventDefault()}',
    )

    def repl(m: re.Match) -> str:
        event = m.group(1)
        mods = m.group(2) or ""
        handler = m.group(3)
        native = EVENT_MAP.get(event, f"on{event}")

        if "preventDefault" in mods and event == "submit":
            h = handler.strip()
            if h.startswith("{") and h.endswith("}"):
                body = h[1:-1].strip()
            else:
                body = f"{h}(e)"
            return f"onsubmit={{(e) => {{ e.preventDefault(); {body}; }}}}"
        if "stopPropagation" in mods:
            h = handler.strip()
            if h.startswith("{") and h.endswith("}"):
                body = h[1:-1].strip()
            else:
                body = h
            return native + "={(e) => { e.stopPropagation(); " + body + "; }}"
        return f"{native}={handler}"

    return re.sub(
        r"\bon:([a-zA-Z]+)((?:\|[a-zA-Z]+)*)\s*=\s*(\{[^}]*\}|[a-zA-Z_][\w.]*)",
        repl,
        content,
    )


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    for rel in FILES:
        p = root / rel
        if not p.exists():
            continue
        old = p.read_text(encoding="utf-8")
        new = convert_events(old)
        if new != old:
            p.write_text(new, encoding="utf-8")
            print(f"events: {rel}")


if __name__ == "__main__":
    main()
