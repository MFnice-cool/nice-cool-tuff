#!/usr/bin/env python3
"""Batch migrate on: handlers and $: reactive statements to Svelte 5 runes."""
import re
import sys
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
    def repl(m: re.Match) -> str:
        event = m.group(1)
        mods = m.group(2) or ""
        handler = m.group(3)
        native = EVENT_MAP.get(event, f"on{event}")

        if "preventDefault" in mods and event == "submit":
            return f'onsubmit={{(e) => {{ e.preventDefault(); {handler.strip()[1:-1] if handler.startswith("{") else handler}(e); }}}}'
        if "preventDefault" in mods and event == "dragover":
            return 'ondragover={(e) => e.preventDefault()}'
        if "stopPropagation" in mods:
            inner = handler.strip()
            if inner.startswith("{") and inner.endswith("}"):
                inner = inner[1:-1].strip()
            return f"onclick={{(e) => {{ e.stopPropagation(); {inner}; }}}}"
        return f"{native}={handler}"

    # on:event|mod1|mod2={handler}
    content = re.sub(
        r"\bon:([a-zA-Z]+)((?:\|[a-zA-Z]+)*)\s*=\s*(\{[^}]*\}|[a-zA-Z_][\w.]*)",
        repl,
        content,
    )
    # bare on:dragover|preventDefault without handler
    content = re.sub(
        r"\bon:dragover\|preventDefault\b",
        'ondragover={(e) => e.preventDefault()}',
        content,
    )
    return content


def find_block_end(lines: list[str], start: int) -> int:
    """Find end of $: statement (handles braces/parens)."""
    depth = 0
    started = False
    buf = lines[start]
    for ch in buf:
        if ch in "({[":
            depth += ch in "({["
            started = True
        elif ch in "})]":
            depth -= max(0, depth - 1)
    if "$:" in buf and not started:
        # single line assignment
        if ";" in buf or not buf.rstrip().endswith((",", "(", "[", "{")):
            # check continuation
            if start + 1 < len(lines):
                nxt = lines[start + 1].strip()
                if nxt and not nxt.startswith("$:") and re.match(r"^[)\]}]|^\w", nxt) is None:
                    if not buf.rstrip().endswith((",", "(", "[", "{", "&&", "||", "?")):
                        return start
            else:
                return start

    i = start
    while i < len(lines):
        line = lines[i]
        for ch in line:
            if ch in "({[":
                depth += 1
                started = True
            elif ch in "})]":
                depth -= 1
        if i > start and depth <= 0 and started:
            return i
        if i == start and ";" in line and depth <= 0:
            return i
        # single-line without semicolon that's complete
        if i == start and depth == 0 and not line.rstrip().endswith((",", "(", "[", "{", "&&", "||", "?")):
            if not re.search(r"\($|\[$|\{$", line.rstrip()):
                return i
        i += 1
        if i >= len(lines):
            return len(lines) - 1
    return i


def convert_reactive_block(lines: list[str], start: int) -> tuple[list[str], int]:
    """Convert one $: block starting at start. Returns new lines and next index."""
    end = find_block_end(lines, start)
    block_lines = lines[start : end + 1]
    first = block_lines[0]
    indent = re.match(r"^(\s*)", first).group(1)
    body = "\n".join(l.strip().removeprefix("$:").strip() for l in block_lines).strip()

    # void call
    if body.startswith("void "):
        new = [
            f"{indent}$effect(() => {{",
            f"{indent}  {body};",
            f"{indent}}});",
        ]
        return new, end + 1

    # function call side effect: foo(...)
    if re.match(r"^[a-zA-Z_]\w*\(", body) and "=" not in body.split("(")[0]:
        new = [
            f"{indent}$effect(() => {{",
            f"{indent}  {body};",
            f"{indent}}});",
        ]
        return new, end + 1

    # if statement
    if body.startswith("if "):
        new = [
            f"{indent}$effect(() => {{",
            f"{indent}  {body}",
            f"{indent}}});",
        ]
        return new, end + 1

    # assignment: name = expr
    m = re.match(r"^(\w+)\s*=\s*", body)
    if m:
        name = m.group(1)
        expr = body[len(m.group(0)) :]
        new = [f"{indent}const {name} = $derived({expr});"]
        return new, end + 1

    # fallback: wrap as effect
    new = [
        f"{indent}$effect(() => {{",
        f"{indent}  {body};",
        f"{indent}}});",
    ]
    return new, end + 1


def convert_reactive(content: str) -> str:
    lines = content.split("\n")
    out: list[str] = []
    i = 0
    while i < len(lines):
        if re.match(r"^\s*\$:", lines[i]):
            new_lines, next_i = convert_reactive_block(lines, i)
            out.extend(new_lines)
            i = next_i
        else:
            out.append(lines[i])
            i += 1
    return "\n".join(out)


def convert_simple_state(content: str) -> str:
    """Convert top-level let assignments to $state in script block."""
    m = re.search(r"(<script[^>]*>)(.*?)(</script>)", content, re.DOTALL)
    if not m:
        return content
    prefix, script, suffix = m.group(1), m.group(2), m.group(3)
    lines = script.split("\n")
    out = []
    skip_patterns = (
        re.compile(r"^\s*(const|type|function|async function|import|export|\}|//|\*|/\*)"),
        re.compile(r"^\s*let \{"),
        re.compile(r"\$props\(\)"),
        re.compile(r"\$state\("),
        re.compile(r"\$derived\("),
        re.compile(r"\$effect\("),
    )
    for line in lines:
        stripped = line.strip()
        if re.match(r"^\s*let\s+\w+", line):
            skip = any(p.search(line) for p in skip_patterns)
            if not skip and "=" in line and not stripped.startswith("let {"):
                # simple one-line assignment
                lm = re.match(r"^(\s*let\s+)(\w+)(\s*:\s*[^=]+)?(\s*=\s*)(.+);?\s*$", line)
                if lm:
                    line = f"{lm.group(1)}{lm.group(2)}{lm.group(3) or ''} = $state({lm.group(5).rstrip(';')});"
        out.append(line)
    new_script = "\n".join(out)
    return content[: m.start()] + prefix + new_script + suffix + content[m.end() :]


def process_file(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    original = text
    text = convert_events(text)
    text = convert_reactive(text)
    if text != original:
        path.write_text(text, encoding="utf-8")
        print(f"Updated {path.name}")


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    for rel in FILES:
        p = root / rel
        if p.exists():
            process_file(p)
        else:
            print(f"Skip missing {rel}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
