#!/usr/bin/env python3
"""Wrap every user-facing Chinese literal in src/*.rs with tr!("…") and emit the
locale skeletons. Keys are the Chinese text itself, so nothing has to be named.

Skipped on purpose: comments, test modules, raw strings (sample JSON), literals
already inside t!/tr!, and format! templates (they need %{name} by hand).
"""
import pathlib
import re
import sys

SRC = pathlib.Path("src")
LOCALES = pathlib.Path("locales")
SKIP_FILES = {"lib.rs", "i18n.rs", "pretty.rs"}

CJK = re.compile(r"[\u3400-\u9fff]")
LITERAL = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
ALREADY = re.compile(r"(?:\w!|\b(?:t|tr|fl))\(\s*$")
FORMAT_HEAD = re.compile(r"\b(?:format|format!|write|writeln|println|print|eprintln|info|warn|error|panic|assert|assert_eq|debug_assert)\s*!?\s*\(\s*$")

found = {}
skipped_format = []
report = []

for path in sorted(SRC.glob("*.rs")):
    if path.name in SKIP_FILES:
        continue
    lines = path.read_text().split("\n")
    out = []
    in_raw = False
    cut = False
    hits = 0
    for lineno, line in enumerate(lines, 1):
        if cut:
            out.append(line)
            continue
        if line.strip() == "#[cfg(test)]":
            cut = True
            out.append(line)
            continue
        stripped = line.lstrip()
        if in_raw:
            out.append(line)
            if "\"#" in line:
                in_raw = False
            continue
        if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
            out.append(line)
            continue
        if 'r#"' in line:
            # a raw string starts here; only end the block when we see its "#
            if "\"#" not in line.split('r#"', 1)[1]:
                in_raw = True
            out.append(line)
            continue

        def repl(m):
            global hits
            text = m.group(1)
            if not CJK.search(text):
                return m.group(0)
            before = line[: m.start()]
            if ALREADY.search(before):
                return m.group(0)
            if FORMAT_HEAD.search(before):
                skipped_format.append(f"{path.name}:{lineno}: \"{text}\"")
                return m.group(0)
            found.setdefault(text, 0)
            found[text] += 1
            hits += 1
            return f'tr!("{text}")'

        out.append(LITERAL.sub(repl, line))
    if hits:
        content = "\n".join(out)
        # every file that now calls tr! needs the macro in scope
        if "use rust_i18n::t;" in content:
            content = content.replace(
                "use rust_i18n::t;", "use rust_i18n::t;\nuse crate::tr;", 1
            )
        elif "\nuse gpui_kit::" not in content:
            content = f"use crate::tr;\n{content}"
        else:
            content = content.replace("\nuse gpui_kit::", "\nuse crate::tr;\nuse gpui_kit::", 1)
        path.write_text(content)
    report.append(f"{path.name}: {hits}")

LOCALES.mkdir(exist_ok=True)
ordered = sorted(found, key=lambda s: s)


def toml_escape(s):
    return s.replace("\\", "\\\\").replace('"', '\\"')


zh = [
    "# Identity map: the key IS the Chinese text, so the zh bundle answers every",
    "# key with its own name. Generated together with the tr!() call sites.",
    "",
]
zh += [f'"{toml_escape(k)}" = "{toml_escape(k)}"' for k in ordered]
en = [
    "# English bundle. Values still holding Chinese are untranslated; a missing",
    "# key also renders Chinese, so a gap is always visible.",
    "",
]
en += [f'"{toml_escape(k)}" = "{toml_escape(k)}"' for k in ordered]
(LOCALES / "auto.zh.toml").write_text("\n".join(zh) + "\n")
(LOCALES / "auto.en.toml").write_text("\n".join(en) + "\n")

print("\n".join(report))
print(f"total literals wrapped: {sum(found.values())}  unique: {len(found)}")
if skipped_format:
    print("\nformat!/assert templates left for hand translation:")
    print("\n".join(skipped_format))
