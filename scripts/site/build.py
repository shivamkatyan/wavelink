#!/usr/bin/env python3
"""Build the Wavelink static site into <repo>/site_build/.

- Renders `index.html` from docs/site/home.html inside the shared template.
- Renders `docs/<slug>.html` for every docs/user/*.md (markdown -> HTML),
  with a docs sub-navigation and breadcrumb.
- Copies docs/site/styles.css.

Deterministic and dependency-light: requires the `markdown` package (pure
Python, MIT) — `build.sh` installs it if missing. All paths resolve from the
repo root regardless of CWD.

Env:
  WDR_SITE_OUT   output dir (default <repo>/site_build)
  WDR_SITE_REPO  "owner/repo" for GitHub links (default: git origin, else
                 shivamkatyan/wavelink)
"""
import html
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SITE = ROOT / "docs" / "site"
SRC_DOCS = ROOT / "docs" / "user"
OUT = Path(os.environ.get("WDR_SITE_OUT", ROOT / "site_build"))
OUT = OUT if OUT.is_absolute() else (ROOT / OUT)

# docs order + (slug, source file). "index" = the docs landing page.
DOCS = [
    ("index", "README.md"),
    ("setup-and-install", "setup-and-install.md"),
    ("getting-started", "getting-started.md"),
    ("free-vs-pro", "free-vs-pro.md"),
    ("bluetooth", "bluetooth.md"),
    ("privacy-and-security", "privacy-and-security.md"),
    ("troubleshooting", "troubleshooting.md"),
    ("platform-support", "platform-support.md"),
]

P_HOME, P_DOCS = "{{HOME_CLASS}}", "{{DOCS_CLASS}}"
_repo_default = "shivamkatyan/wavelink"


def repo_from_origin() -> str:
    try:
        url = subprocess.check_output(
            ["git", "config", "--get", "remote.origin.url"],
            cwd=ROOT, text=True, stderr=subprocess.DEVNULL,
        ).strip()
    except Exception:
        return _repo_default
    m = re.search(r"([^/:]+)/([^/:]+?)(?:\.git)?$", url)
    return f"{m.group(1)}/{m.group(2)}" if m else _repo_default


def branch_from_git() -> str:
    """Current branch for GitHub blob/tree links (default 'main')."""
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--abbrev-ref", "HEAD"],
            cwd=ROOT, text=True, stderr=subprocess.DEVNULL,
        ).strip() or "main"
    except Exception:
        return "main"


def humanize(slug: str) -> str:
    return slug.replace("-", " ").title()


def load_template() -> str:
    t = (SITE / "template.html").read_text()
    placeholders = re.findall(r"\{\{[A-Z_]+\}\}", t)
    missing = [p for p in placeholders if p not in {
        "{{TITLE}}", "{{DESC}}", "{{BASE}}", "{{HOME_CLASS}}",
        "{{DOCS_CLASS}}", "{{BODY}}", "{{REPO}}", "{{BRANCH}}"}]
    if missing:
        raise SystemExit(f"template placeholders not handled: {sorted(set(missing))}")
    return t


def first_heading(text: str) -> str:
    for line in text.splitlines():
        if line.startswith("# "):
            return line[2:].strip()
    return "Untitled"


def first_para(text: str) -> str:
    for line in text.splitlines():
        if line.strip() and not line.startswith(("#", "```", ">", "|", "-")):
            return re.sub(r"[*_`\[\]]", "", line).strip()
    return ""


def render(title: str, desc: str, base: str, home_class: str,
           docs_class: str, repo: str, branch: str) -> str:
    t = load_template()
    return (t
            .replace("{{TITLE}}", html.escape(f"{title} — Wavelink"))
            .replace("{{DESC}}", html.escape(desc))
            .replace("{{BASE}}", base)
            .replace(P_HOME, home_class)
            .replace(P_DOCS, docs_class)
            .replace("{{REPO}}", repo)
            .replace("{{BRANCH}}", branch))


def docs_nav(active: str, repo: str) -> str:
    items = []
    for slug, _ in DOCS:
        label = "Overview" if slug == "index" else humanize(slug)
        cur = ' aria-current="page"' if slug == active else ""
        href = "index.html" if slug == "index" else f"{slug}.html"
        items.append(f'<a href="{href}"{cur}>{html.escape(label)}</a>')
    return "".join(f'  <nav class="docs-nav" aria-label="Sections">{items}</nav>')


# src file name -> site slug, for sibling-doc .md -> .html rewriting.
DOC_BY_SRC = {src: slug for slug, src in DOCS}


def fix_links(body_html: str, repo: str, branch: str, src_md: str) -> str:
    """Make rendered markdown links site-correct.

    - Known sibling docs: `href="X.md"` -> `href="X.html"` (site pages).
    - Any other relative `../...md` link: resolve against docs/user/<src_md>
      and point at the GitHub blob (current branch) instead of a non-existent
      site page.
    """
    for src, slug in DOC_BY_SRC.items():
        body_html = body_html.replace(f'href="{src}"', f'href="{slug}.html"')
        body_html = body_html.replace(f'href="{src}#', f'href="{slug}.html#')
    def _repl(m: "re.Match[str]") -> str:
        href = m.group(1)
        if href.startswith(("http", "mailto:", "#")) or not href.endswith(".md"):
            return m.group(0)
        rel = (SRC_DOCS / src_md).parent / href
        try:
            repo_path = rel.resolve().relative_to(ROOT)
        except ValueError:
            return m.group(0)
        return f'href="https://github.com/{repo}/blob/{branch}/{repo_path}"'
    return re.sub(r'href="([^"]+)"', _repl, body_html)


def main() -> int:
    try:
        import markdown as mdlib  # noqa: PLC0415
    except ImportError:
        print("error: 'markdown' module missing — run: python3 -m pip install markdown", file=sys.stderr)
        return 1

    repo = os.environ.get("WDR_SITE_REPO") or repo_from_origin()
    branch = os.environ.get("WDR_SITE_BRANCH") or branch_from_git()

    (OUT / "docs").mkdir(parents=True, exist_ok=True)

    # --- home -------------------------------------------------------------
    home_body = (SITE / "home.html").read_text().replace(
        "shivamkatyan/wavelink", repo)
    home_html = render(
        title="Hi-Fi audio over Wi-Fi to a portable DAC",
        desc="Send a computer's live audio over local Wi-Fi to a phone or tablet with a USB "
             "DAC — Free lossy and honest, measured Pro lossless.",
        base=".", home_class=' aria-current="page"', docs_class="", repo=repo, branch=branch)
    (OUT / "index.html").write_text(home_html.replace("{{BODY}}", home_body))
    print(f"built  index.html ({len(home_body)} bytes home body)")

    # --- docs -------------------------------------------------------------
    for slug, src in DOCS:
        md = (SRC_DOCS / src).read_text()
        title = first_heading(md)
        body_html = mdlib.markdown(md, extensions=["tables", "fenced_code", "sane_lists"])
        body_html = fix_links(body_html, repo, branch, src)
        desc = first_para(md)
        crumb = f'<p class="crumb"><a href="index.html">Docs</a> · {html.escape(title)}</p>'
        page_html = render(
            title=title, desc=desc, base="..",
            home_class="", docs_class=' aria-current="page"', repo=repo, branch=branch)
        page_html = page_html.replace("{{BODY}}", crumb + docs_nav(slug, repo) + body_html)
        (OUT / "docs" / f"{slug}.html").write_text(page_html)
        print(f"built  docs/{slug}.html ({title})")

    shutil.copyfile(SITE / "styles.css", OUT / "styles.css")
    print(f"copied styles.css -> {OUT.relative_to(ROOT) if OUT.is_relative_to(ROOT) else OUT}/styles.css")
    print(f"done. site at {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
