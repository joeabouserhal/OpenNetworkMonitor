"""Render exported egui test meshes offscreen. Requires numpy and Pillow; no desktop input.

cargo test --features ui-preview export_ui_review -- --ignored
python scripts/render_ui.py
"""
from pathlib import Path
import json
import numpy as np
from PIL import Image

root = Path(__file__).resolve().parents[1] / 'target' / 'ui-review'
for source in sorted(root.glob('*.json')):
    data = json.loads(source.read_text(encoding='utf-8'))
    width, height = map(int, data['size'])
    canvas = np.zeros((height, width, 4), dtype=np.float32)
    textures = {t['id']: np.frombuffer((root / t['file']).read_bytes(), dtype=np.uint8).reshape(t['size'][1], t['size'][0], 4).astype(np.float32) / 255 for t in data['textures']}
    for mesh in data['meshes']:
        texture = textures[mesh['texture']]
        th, tw = texture.shape[:2]
        pos = np.array([[v[0], v[1]] for v in mesh['vertices']], dtype=np.float32)
        uv = np.array([[v[2], v[3]] for v in mesh['vertices']], dtype=np.float32)
        colors = np.array([v[4] for v in mesh['vertices']], dtype=np.float32) / 255
        clip = mesh['clip']
        for ids in np.array(mesh['indices']).reshape(-1, 3):
            p = pos[ids]
            lo = np.maximum(np.floor(p.min(axis=0)).astype(int), [max(0, int(np.ceil(clip[0]))), max(0, int(np.ceil(clip[1])))])
            hi = np.minimum(np.ceil(p.max(axis=0)).astype(int), [min(width, int(np.floor(clip[2]))), min(height, int(np.floor(clip[3])))])
            if np.any(hi <= lo): continue
            x = np.arange(lo[0], hi[0], dtype=np.float32)[None, :] + .5
            y = np.arange(lo[1], hi[1], dtype=np.float32)[:, None] + .5
            denominator = (p[1, 1]-p[2, 1])*(p[0, 0]-p[2, 0])+(p[2, 0]-p[1, 0])*(p[0, 1]-p[2, 1])
            if abs(denominator) < 1e-8: continue
            a = ((p[1, 1]-p[2, 1])*(x-p[2, 0])+(p[2, 0]-p[1, 0])*(y-p[2, 1]))/denominator
            b = ((p[2, 1]-p[0, 1])*(x-p[2, 0])+(p[0, 0]-p[2, 0])*(y-p[2, 1]))/denominator
            c = 1-a-b
            mask = (a >= 0) & (b >= 0) & (c > 0)
            if not mask.any(): continue
            weights = np.stack(np.broadcast_arrays(a, b, c), axis=-1)[mask]
            coords = weights @ uv[ids] * [tw, th] - .5
            coords[:, 0] = np.clip(coords[:, 0], 0, tw-1)
            coords[:, 1] = np.clip(coords[:, 1], 0, th-1)
            xy = np.floor(coords).astype(int)
            frac = coords-xy
            xx, yy = xy[:, 0], xy[:, 1]
            fx, fy = frac[:, 0, None], frac[:, 1, None]
            sampled = ((texture[yy, xx]*(1-fx)+texture[yy, np.minimum(xx+1, tw-1)]*fx)*(1-fy)
                       +(texture[np.minimum(yy+1, th-1), xx]*(1-fx)+texture[np.minimum(yy+1, th-1), np.minimum(xx+1, tw-1)]*fx)*fy)
            ink = sampled * (weights @ colors[ids])
            region = canvas[lo[1]:hi[1], lo[0]:hi[0]]
            region[mask] = ink + region[mask]*(1-ink[:, 3, None])
    alpha = canvas[:, :, 3:4]
    canvas[:, :, :3] = np.divide(canvas[:, :, :3], alpha, out=np.zeros_like(canvas[:, :, :3]), where=alpha>0)
    Image.fromarray((np.clip(canvas, 0, 1)*255+.5).astype(np.uint8)).save(source.with_suffix('.png'))
    print(source.with_suffix('.png').name)
