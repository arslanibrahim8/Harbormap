#!/usr/bin/env python3
"""Check technology pack keys against public package registries.

Usage: python3 -I scripts/tech/registry_check.py <pack.toml> [<pack.toml>...]

Prints one tab-separated line per key:
  tech_id<TAB>ecosystem<TAB>key<TAB>STATUS<TAB>upstream description
then a summary line. Exit code is always 0.
"""

import html
import json
import re
import sys
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request

USER_AGENT = "harbormap-pack-check (https://github.com/harbormap/harbormap)"
TIMEOUT = 15
ECOSYSTEMS = ("npm", "pypi", "go", "maven", "nuget", "crates")


class Missing(Exception):
    """The registry says the key does not exist."""


class CheckError(Exception):
    """An expected failure; the message is the reason printed after ERROR."""


def clean(value):
    """Collapse all whitespace so each output field stays on one line."""
    return " ".join(str(value or "").split())


def fetch(url):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as exc:
        if exc.code in (404, 410):
            raise Missing() from None
        raise CheckError(f"HTTP {exc.code}") from None
    except urllib.error.URLError as exc:
        if isinstance(exc.reason, TimeoutError):
            raise CheckError("timeout") from None
        raise CheckError(f"network: {exc.reason}") from None
    except TimeoutError:
        raise CheckError("timeout") from None


def fetch_json(url):
    return json.loads(fetch(url))


def with_canonical(key, canonical, desc):
    """Prefix the description when the registry's canonical name differs from the key."""
    canonical = clean(canonical)
    if canonical and canonical != key:
        return f"canonical name: {canonical} · {desc}" if desc else f"canonical name: {canonical}"
    return desc


def check_npm(name):
    data = fetch_json("https://registry.npmjs.org/" + name.replace("/", "%2F") + "/latest")
    return "OK", clean(data.get("description"))


def check_pypi(name):
    data = fetch_json("https://pypi.org/pypi/" + urllib.parse.quote(name, safe="") + "/json")
    info = data.get("info") or {}
    return "OK", with_canonical(name, info.get("name"), clean(info.get("summary")))


def go_escape(module):
    """Go module proxy case encoding: each uppercase letter X becomes !x."""
    return re.sub(r"[A-Z]", lambda m: "!" + m.group(0).lower(), module)


def check_go(module):
    path = urllib.parse.quote(go_escape(module), safe="/")
    fetch(f"https://proxy.golang.org/{path}/@latest")
    return "OK", f"see https://pkg.go.dev/{module}"


def check_maven(key):
    group, _, artifact = key.partition(":")
    g = urllib.parse.quote(group, safe="")
    a = urllib.parse.quote(artifact, safe="")
    # Quotes are sent as %22: the server rejects raw double quotes with HTTP 400.
    search = f"https://search.maven.org/solrsearch/select?q=g:%22{g}%22+AND+a:%22{a}%22&rows=1&wt=json"
    docs = (fetch_json(search).get("response") or {}).get("docs") or []
    if not docs:
        raise Missing()
    version = docs[0].get("latestVersion")
    if not version:
        raise CheckError("search result has no latestVersion")
    pom_url = (
        f"https://repo1.maven.org/maven2/{group.replace('.', '/')}/{artifact}"
        f"/{version}/{artifact}-{version}.pom"
    )
    try:
        pom = fetch(pom_url)
    except Missing:
        raise CheckError("pom not found") from None
    match = re.search(r"<description>(.*?)</description>", pom, re.S)
    desc = clean(html.unescape(match.group(1))) if match else ""
    return "OK", desc


def check_nuget(package_id):
    url = (
        "https://azuresearch-usnc.nuget.org/query?q=packageid:"
        + urllib.parse.quote(package_id, safe="")
        + "&take=1&prerelease=false"
    )
    data = fetch_json(url).get("data") or []
    if not data:
        raise Missing()
    desc = clean(data[0].get("description"))[:200]
    return "OK", with_canonical(package_id, data[0].get("id"), desc)


def check_crates(name):
    data = fetch_json("https://crates.io/api/v1/crates/" + urllib.parse.quote(name, safe=""))
    crate = data.get("crate") or {}
    return "OK", with_canonical(name, crate.get("name"), clean(crate.get("description")))


def check_image(name):
    first, _, rest = name.partition("/")
    if rest:
        ns, repo = first, rest
    else:
        ns, repo = "library", first
    url = (
        "https://hub.docker.com/v2/repositories/"
        f"{urllib.parse.quote(ns, safe='')}/{urllib.parse.quote(repo, safe='')}/"
    )
    data = fetch_json(url)
    return "OK", clean(data.get("description"))


CHECKS = {
    "npm": check_npm,
    "pypi": check_pypi,
    "go": check_go,
    "maven": check_maven,
    "nuget": check_nuget,
    "crates": check_crates,
    "image": check_image,
}


def is_pattern(ecosystem, key):
    if key.endswith("*"):
        return True
    if ecosystem == "image":
        first = key.split("/", 1)[0]
        return "." in first or ":" in first  # registry host, not Docker Hub
    return False


def check_key(ecosystem, key):
    try:
        status, desc = CHECKS[ecosystem](key)
    except Missing:
        return "MISSING", "-"
    except CheckError as exc:
        return f"ERROR {clean(exc)}", "-"
    except Exception as exc:  # one bad key must never stop the run
        return clean(f"ERROR {type(exc).__name__}: {exc}"), "-"
    return status, desc or "-"


def load_entries(path):
    with open(path, "rb") as fh:
        doc = tomllib.load(fh)
    entries = []
    for tech in doc.get("tech", []):
        if not isinstance(tech, dict):
            continue
        tid = tech.get("id", "?")
        for ecosystem in ECOSYSTEMS:
            for key in tech.get(ecosystem) or []:
                entries.append((tid, ecosystem, str(key)))
    for image in doc.get("image", []):
        if not isinstance(image, dict):
            continue
        tid = image.get("tech", "?")
        for name in image.get("names") or []:
            entries.append((tid, "image", str(name)))
    return entries


def main(paths):
    if not paths:
        print("usage: registry_check.py <pack.toml> [<pack.toml>...]", file=sys.stderr)
        return
    counts = {"OK": 0, "MISSING": 0, "PATTERN": 0, "ERROR": 0}
    crates_used = False
    for path in paths:
        try:
            entries = load_entries(path)
        except (OSError, tomllib.TOMLDecodeError) as exc:
            print(f"{path}: cannot read pack: {exc}", file=sys.stderr)
            continue
        for tid, ecosystem, key in entries:
            if is_pattern(ecosystem, key):
                status, desc = "PATTERN", "-"
            else:
                if ecosystem == "crates":
                    if crates_used:
                        time.sleep(1)  # be polite to crates.io
                    crates_used = True
                status, desc = check_key(ecosystem, key)
            if status.startswith("ERROR"):
                counts["ERROR"] += 1
            elif status in counts:
                counts[status] += 1
            fields = (tid, ecosystem, key, status, desc)
            print("\t".join(clean(f) for f in fields), flush=True)
    print(
        f"summary: {counts['OK']} ok, {counts['MISSING']} missing, "
        f"{counts['PATTERN']} patterns, {counts['ERROR']} errors",
        flush=True,
    )


if __name__ == "__main__":
    main(sys.argv[1:])
    sys.exit(0)
