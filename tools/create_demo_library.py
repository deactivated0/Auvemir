#!/usr/bin/env python3
"""Create explicitly synthetic audio and cover art for repeatable native UI captures."""
import argparse, math, subprocess
from pathlib import Path
from PIL import Image, ImageDraw

ALBUMS = [
    ("Evening Current", (40,92,100), (188,220,173), ["Drift", "A Softer Tide", "Night Garden"]),
    ("Quiet Geometry", (63,60,96), (210,174,153), ["Open Lines", "Small Hours", "Still Forms"]),
    ("Blue Hours", (33,59,90), (133,204,205), ["After Rain", "Window Light", "Slow Motion"]),
    ("Golden Room", (115,65,43), (227,196,112), ["Warmth", "Sunday", "Last Light"]),
]
def create(destination):
    destination=Path(destination);destination.mkdir(parents=True,exist_ok=True)
    for album_index,(album, low, high, titles) in enumerate(ALBUMS):
        folder=destination/album;folder.mkdir(exist_ok=True)
        cover=folder/"cover.png"
        if not cover.exists():
            art=Image.new("RGB",(512,512));pixels=art.load()
            for y in range(512):
                for x in range(512):
                    t=min(1,max(0,(x+y)/1024 + math.sin(x/110)*.08))
                    pixels[x,y]=tuple(round(a+(b-a)*t) for a,b in zip(low,high))
            draw=ImageDraw.Draw(art)
            for i in range(8):
                r=50+i*25;draw.ellipse((256-r,256-r,256+r,256+r),outline=tuple(min(255,c+20) for c in high),width=2)
            draw.rectangle((32,32,36,110),fill=high)
            art.save(cover,optimize=True)
        for i,title in enumerate(titles):
            path=folder/f"{i+1:02} - {title}.flac"
            if path.exists(): continue
            subprocess.run(["ffmpeg","-nostdin","-hide_banner","-loglevel","error","-y","-f","lavfi","-i",f"sine=frequency={220+album_index*55+i*22}:duration=45:sample_rate=44100","-af","volume=0.015","-c:a","flac","-metadata",f"title={title}","-metadata","artist=Auvemir Studio (demo)","-metadata",f"album={album}","-metadata",f"track={i+1}",str(path)],check=True)
    return destination
if __name__ == "__main__":
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("destination",type=Path);args=parser.parse_args();print(create(args.destination))
