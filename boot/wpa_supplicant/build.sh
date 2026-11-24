# Builds a static libnl, then wpa_supplicant against it (see BUILD.bazel).
set -eu
top=$PWD
mkdir libnl wpa
tar -xf /in/src/libnl.tar.gz -C libnl --strip-components=1
tar -xf /in/src/wpa_supplicant.tar.gz -C wpa --strip-components=1
(
    cd libnl
    ./configure --prefix="$top/sysroot" --disable-shared --enable-static --disable-cli --disable-debug >/dev/null
    make -j4 >/dev/null
    make install >/dev/null
)
cp /in/src/wpa_supplicant.config wpa/wpa_supplicant/.config
cd wpa/wpa_supplicant
# drivers.mak lists -lnl-3 before -lnl-genl-3; a static link needs it again after.
make -j4 V=1 LIBNL_INC="$top/sysroot/include/libnl3" LDFLAGS="-static -L$top/sysroot/lib" \
    EXTRALIBS="-lnl-3 -lpthread -lm" wpa_supplicant wpa_cli
strip wpa_supplicant wpa_cli
