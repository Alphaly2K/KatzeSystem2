# MPEG 合成片段

`clip.mpg` 包含四幅 32×32 视频帧，上半幅 RGB 为 (220,30,30)，下半幅为 (30,30,220)。帧率为 25，GOP 为 2，启用一幅 B 帧。音频为 48 kHz 双声道，左声道 1 kHz、幅度 8000，右声道 600 Hz、幅度 6000，输入共 7680 采样帧，按有符号 16 位整数生成。

使用 FFmpeg 的 mpeg1video（q=2）及 MP2（192 kbps）编码，封装为 MPEG Program Stream。封装后将首个系统头的六位 audio_bound 清零，保留实际音频 PES；用于回归音轨探测。片段由上述几何和正弦波生成，不含游戏或第三方媒体素材。常规测试直接读取已保存字节，无需编码器。

`clip-mp3.mpg` 与 `clip-mp3-mono.mpg` 保留同一片段的视频，将其合成音频重采样为 44.1 kHz，并使用 libmp3lame 编码。前者为双声道 VBR（质量 4），后者为单声道 CBR（96 kbps）；两者均封装为 MPEG Program Stream，不写入 Xing 头，用于流式 MP3、单声道输出及重播状态回归。
