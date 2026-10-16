`chapter-base.m4a` is a two-second, 440 Hz AAC tone generated with FFmpeg's
`sine` source; it contains no voice or third-party recording. The chapter tests
copy it to a temporary `.m4b` and add Nero chapter-list or QuickTime chapter-track
metadata with the pinned `mp4ameta` writer before testing the actual reader.

To regenerate the base file from this directory:

```sh
ffmpeg -hide_banner -loglevel error -f lavfi -i sine=frequency=440:duration=2 \
  -c:a aac -b:a 32k -movflags +faststart -y chapter-base.m4a
```
