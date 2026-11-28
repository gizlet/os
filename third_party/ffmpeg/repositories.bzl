"""FFmpeg release archives from ffmpeg.org."""

load("//third_party:pinned_files.bzl", "pinned_files_extension")

# The release Debian trixie's gstreamer1.0-libav is built against, so Debian's
# plugin runs on the runtime's own FFmpeg. The archive's signature checks with
# FFmpeg's release key FCF986EA15E6E293A5644F10B4322F04D67658D8.
ffmpeg = pinned_files_extension(
    name = "ffmpeg",
    files = {
        "ffmpeg-7.1.5.tar.xz": ["https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz", "de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f"],
    },
    doc = "Provides the pinned FFmpeg source archive.",
)
