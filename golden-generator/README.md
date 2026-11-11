# golden-generator

Generates and verifies the golden fixtures under `crates/*/tests/fixtures/`
with Constellation's own code. It is **not** a workspace member: it is built
inside a copy of the Constellation workspace at the commit named in each
crate's PROVENANCE.md (it is the one place this repository names that
project's crates).

```sh
# 1. a copy of the source commit's tree (do not build in the original checkout)
rsync -a --exclude target/ --exclude .git/ <constellation checkout>/ ~/constellation-src/
cp -r golden-generator ~/constellation-src/crates/golden-gen
cd ~/constellation-src
sed -i '0,/"crates\/types",/s//"crates\/types", "crates\/golden-gen",/' Cargo.toml

# 2. origin -> replicix: regenerate the fixtures
cargo run -p golden-gen -- gen /tmp/golden-out
cp -r /tmp/golden-out/chunkfmt/.    <replicix-core>/crates/chunkfmt/tests/fixtures/
cp -r /tmp/golden-out/objcas/.      <replicix-core>/crates/objcas/tests/fixtures/
cp -r /tmp/golden-out/csi-common/.  <replicix-core>/crates/csi-common/tests/fixtures/

# 3. replicix -> origin: write what replicix-core produces, verify it with Constellation's code
cd <replicix-core>
for c in chunkfmt objcas csi-common; do
  cargo run -p replicix-$c --example golden_out -- /tmp/rep-out/$c
done
cd ~/constellation-src && cargo run -p golden-gen -- verify /tmp/rep-out
```

`gen` fails on its own assertions if Constellation's parser accepts an id the fixtures call bad; `verify` panics on the first difference.
