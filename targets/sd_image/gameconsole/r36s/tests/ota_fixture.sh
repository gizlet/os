#!/bin/sh
set -eu
cd /tmp/updates
work=$PWD/ota
mkdir -p "$work/bin"
base=/usr/lib/gizlet/launcher
printf 'launcher namespaces="gizlet-launcher" %s\n' "$(cat test-key.pub)" > "$work/allowed_signers"
mount --bind "$work/allowed_signers" "$base/allowed_signers"
cat > "$work/bin/curl" <<'EOF'
#!/bin/sh
exec /usr/bin/curl -q --connect-to dl.gizlet.io:443:127.0.0.1:8444 --noproxy '*' "$@"
EOF
chmod 755 "$work/bin/curl"
cd "$work"
openssl req -x509 -newkey ed25519 -nodes -days 1 -subj /CN=dl.gizlet.io \
    -addext subjectAltName=DNS:dl.gizlet.io -keyout tls.key -out tls.crt
cp ../pack.sh pack.sh
for version in 2.0.0 2.0.1; do
    dir=$work/releases/v$version
    mkdir -p "$dir/runtime" "$dir/launcher"
    runtime=$dir/runtime/gizlet-runtime-1.sqfs
    bundle=$dir/launcher/gizlet-launcher-$version.gizlet
    cp /opt/gizlet/runtime/1.sqfs "$runtime"
    printf '%s\n' "$version" >> "$runtime"
    if [ "$version" = 2.0.0 ]; then
        cat > start.sh <<'EOF'
#!/bin/sh
if [ -L /run/gizlet-ota-trial ]; then
    /usr/lib/gizlet/gizlet-ota-boot confirm || exit
fi
echo ota-confirmed
EOF
    else
        printf '#!/bin/sh\necho ota-launch-failed\nexit 1\n' > start.sh
    fi
    sh pack.sh "$version" ../test-key "$base/bin/gizlet-launcher" "$base/bin/cli" "$bundle"
    runtime_sha=$(sha256sum "$runtime")
    bundle_sha=$(sha256sum "$bundle")
    cat > "$dir/manifest.json" <<EOF
{"schema_version":1,"tag":"v$version","ota":{"protocol":1,"arch":"aarch64","runtime_abi":1,
"runtime":{"path":"runtime/gizlet-runtime-1.sqfs","bytes":$(stat -c %s "$runtime"),"sha256":"${runtime_sha%% *}"},
"launcher":{"path":"launcher/gizlet-launcher-$version.gizlet","bytes":$(stat -c %s "$bundle"),"sha256":"${bundle_sha%% *}"}}}
EOF
    ssh-keygen -Y sign -f ../test-key -n gizlet-launcher "$dir/manifest.json"
done
cat > serve.js <<'EOF'
const server = Bun.serve({
  port: 8444,
  hostname: "127.0.0.1",
  idleTimeout: 0,
  tls: { key: Bun.file("tls.key"), cert: Bun.file("tls.crt") },
  async fetch(request) {
    const path = new URL(request.url).pathname;
    if (path === "/") return new Response("ready");
    if (path === "/releases/latest/manifest.json") {
      const tag = (await Bun.file("latest").text()).trim();
      return Response.redirect(`https://dl.gizlet.io/releases/${tag}/manifest.json`);
    }
    const fault = (await Bun.file("fault").text()).trim();
    if (fault === "signature" && path.endsWith("manifest.json.sig")) return new Response("invalid");
    if (fault === "interrupt" && path.endsWith(".sqfs")) {
      return new Response(new ReadableStream({
        start(controller) {
          controller.enqueue(new Uint8Array(4096));
          setTimeout(() => controller.error(new Error("interrupted")), 50);
        },
      }), { headers: { "content-length": "65536" } });
    }
    if (fault === "checksum" && path.endsWith(".sqfs")) return new Response("corrupt");
    const file = Bun.file(`.${path}`);
    if (!await file.exists()) return new Response(null, { status: 404 });
    return new Response(file.stream(), { headers: { "content-length": String(file.size) } });
  },
});
console.log(server.url.href);
EOF
printf 'v2.0.0\n' > latest
: > fault
