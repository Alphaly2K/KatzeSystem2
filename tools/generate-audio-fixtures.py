"""使用 FFmpeg 的 libvorbis 生成合成信号及短来源的独立解码参考。"""

from pathlib import Path
import subprocess


def run(*args):
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args], check=True)


directory = Path(__file__).resolve().parents[1] / "tests" / "fixtures" / "audio"
directory.mkdir(exist_ok=True)
for name, rate, frames, signal, reference in (
    ("mono-44100", 44100, 1379, "0.35*sin(2*PI*440*n/44100)", True),
    (
        "stereo-48000",
        48000,
        1903,
        "0.25*sin(2*PI*440*n/48000)|0.5*sin(2*PI*1300*n/48000)",
        True,
    ),
    (
        "mono-8000-long",
        8000,
        40_000,
        "10000/32768*sin(2*PI*(220+80*n/40000)*n/8000)",
        False,
    ),
):
    encoded = directory / f"{name}.ogg"
    run(
        "-f", "lavfi", "-i", f"aevalsrc={signal}:s={rate}",
        "-af", f"atrim=end_sample={frames}",
        "-c:a", "libvorbis", "-q:a", "4", "-map_metadata", "-1",
        "-fflags", "+bitexact", "-flags:a", "+bitexact", str(encoded),
    )
    if not reference:
        continue
    # 这两种编码配置的预滚为 128 帧；手动裁剪可保留完整的末页。
    run(
        "-flags2", "+skip_manual", "-i", str(encoded),
        "-af", f"atrim=start_sample=128:end_sample={128 + frames}",
        "-c:a", "pcm_s16le", "-f", "s16le", str(directory / f"{name}.pcm"),
    )
