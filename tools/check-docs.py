#!/usr/bin/python3
"""Check repository Markdown file links and GitHub-style heading fragments.
Read-only, standard library only; external URLs are not fetched.
"""
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit


def prose(text):
    lines = []
    fence = None
    for line in text.splitlines():
        match = re.match(r'^\s*(`{3,}|~{3,})', line)
        if match:
            token = match.group(1)
            if fence is None:
                fence = token[0]
            elif fence == token[0]:
                fence = None
            continue
        if fence is None:
            lines.append(line)
    return '\n'.join(lines)


def anchors(text):
    result = set()
    counts = {}
    for line in prose(text).splitlines():
        match = re.match(r'^\s{0,3}#{1,6}\s+(.+?)\s*#*$', line)
        if not match:
            continue
        title = re.sub(r'<[^>]+>', '', match.group(1)).lower()
        slug = re.sub(r'[^\w\- ]', '', title).replace(' ', '-')
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        result.add(f'{slug}-{count}' if count else slug)
    result.update(re.findall(r'<a\s+(?:id|name)=[\"\']([^\"\']+)', text))
    return result


def check_file(path, root):
    failures = []
    text = prose(path.read_text())
    links = re.findall(r'!?\[[^\]]*\]\(([^\n)]+)\)', text)
    links += re.findall(r'^\s*\[[^\]]+\]:\s*(\S+)', text, re.MULTILINE)
    for raw in links:
        target = raw.strip()
        if target.startswith('<'):
            target = target[1:target.index('>')]
        else:
            target = target.split()[0]
        parsed = urlsplit(target)
        if parsed.scheme or parsed.netloc:
            continue
        destination = (root / unquote(parsed.path).lstrip('/') if parsed.path.startswith('/')
                       else path.parent / unquote(parsed.path)) if parsed.path else path
        if not destination.exists():
            failures.append(f'{path.relative_to(root)}: missing target {target}')
        elif parsed.fragment and destination.suffix == '.md':
            if unquote(parsed.fragment) not in anchors(destination.read_text()):
                failures.append(f'{path.relative_to(root)}: missing heading {target}')
    return failures


def main():
    root = Path(__file__).resolve().parents[1]
    names = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '--', '*.md'],
        cwd=root, text=True,
    ).splitlines()
    failures = [error for name in sorted(set(names)) for error in check_file(root / name, root)]
    if failures:
        print('\n'.join(failures), file=sys.stderr)
        return 1
    print(f'Checked {len(set(names))} Markdown files: local links and headings passed.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
