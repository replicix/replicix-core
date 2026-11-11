#!/usr/bin/env python3
"""Lists the `use` statements in a Constellation checkout that import
something replicix-core now provides, grouped by moved crate and by
Constellation crate. Re-run it on the checkout you migrate (its main moves
daily); inline paths (not `use` statements) are found with the greps in
MIGRATION.md.  usage: migration_sites.py <constellation checkout>"""
import collections, os, re, sys

root = sys.argv[1]
files = []
for d, dirs, fs in os.walk(root):
    rel = os.path.relpath(d, root)
    if rel.startswith(("vendor", "target", ".git")):
        dirs[:] = []
        continue
    files += [os.path.join(rel, f) for f in fs if f.endswith(".rs")]


def stmts(text):
    out, lines, i = [], text.split("\n"), 0
    while i < len(lines):
        if re.match(r"\s*(pub(\([a-z]+\))? )?use ", lines[i]):
            j, s = i, lines[i]
            while ";" not in lines[j] and j + 1 < len(lines):
                j += 1
                s += " " + lines[j].strip()
            out.append((i + 1, re.sub(r"\s+", " ", s.strip())))
            i = j
        i += 1
    return out


MOVED = (
    "amazon_s3_builder amazon_s3_builder_resolved amazon_s3_builder_resolved_with configure_s3_client "
    "S3Resolution SuppliedCredentials classify classify_chain classify_message ErrorClass fence_retired "
    "live_leases_held_by AckPolicy InheritedTail Lease LeaseMode LeaseStore LeaseTag RenewalKey "
    "probe_cas_semantics CasProbe CasProbeReport Versioning Capabilities PreflightCheck now_unix_ms "
    "Codec CompressionSetting create_keyring_block unlock Argon2Params E2eKeys KeyringBlock SharedE2eKeys "
    "cas aws_auth probe lease control codec format e2e"
).split()
MOVED_RE = re.compile(r"\b(" + "|".join(MOVED) + r")\b")

res = collections.defaultdict(lambda: collections.defaultdict(list))
for f in files:
    crate = f.split("/")[1] if f.startswith("crates/") else f.split("/")[0]
    for ln, s in stmts(open(os.path.join(root, f), errors="ignore").read()):
        body = s.split("constellation_store_s3", 1)[-1] if "constellation_store_s3" in s else s
        if ("constellation_store_s3" in s or (crate == "store-s3" and "crate::" in s)) and MOVED_RE.search(body):
            res["store-s3 -> replicix-objcas / replicix-chunkfmt"][crate].append((f, ln, s))
        if "constellation_fs_core" in s and re.search(r"ChunkHash|ChunkLayout|ChunkSlice|chunk::", s):
            res["fs-core chunk -> replicix-chunkfmt"][crate].append((f, ln, s))
        if "constellation_platform" in s and re.search(
            r"FileLock|LockGuard|FsPrimitives|forbid_core_dumps|core_dumps_forbidden|lock::|fs::|\{[^}]*\bfs\b", s
        ):
            res["platform -> replicix-platform-lite"][crate].append((f, ln, s))
        if crate == "platform" and re.search(r"crate::(lock|fs)\b|crate::\{", s):
            res["platform -> replicix-platform-lite"][crate].append((f, ln, s))
        if crate == "csi" and re.search(r"(crate|super)::(volume_id|params|credentials|proto)\b|constellation_csi::(volume_id|params|credentials|proto)", s):
            res["csi -> replicix-csi-common"][crate].append((f, ln, s))
        if crate != "csi" and "constellation_csi" in s and re.search(r"volume_id|params|credentials|proto", s):
            res["csi -> replicix-csi-common"][crate].append((f, ln, s))
        if "constellation_upload_concurrency" in s:
            res["upload-concurrency -> replicix-upload-concurrency"][crate].append((f, ln, s))
for g, d in res.items():
    print("##", g)
    for c, v in sorted(d.items()):
        print(f"# {c}: {len(v)} statements in {len({x[0] for x in v})} files")
        for f, ln, s in v:
            print(f"{f}:{ln}: {s}")
