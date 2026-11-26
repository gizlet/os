"""Host execution toolchain for tests of cross-compiled package artifacts.

Derived from GizOS tools/bazel/platforms at 0ac3bfdc2bafa78bfc89965931b5ce22faeff795 (Apache-2.0, see ../rockchip/LICENSE).
"""

def _empty_test_toolchain_impl(_ctx):
    return [platform_common.ToolchainInfo()]

empty_test_toolchain = rule(
    implementation = _empty_test_toolchain_impl,
)
