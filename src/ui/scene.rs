//! Offscreen capture of a scene's egui geometry for visual review. `scripts/render_ui.py`
//! turns the exported files into PNGs; no window or desktop input is involved.
use super::tokens::{self, Tokens};
use eframe::egui;
use std::{collections::HashMap, path::PathBuf};

/// Scripted pointer input applied before a frame is captured.
pub enum Step {
    Click(&'static str),
    ClickAt(f32, f32),
    Hover(f32, f32),
}

type Atlas = HashMap<String, ([usize; 2], Vec<[u8; 4]>)>;

fn apply_textures(textures: &mut Atlas, out: &mut egui::FullOutput) {
    for (id, deltas) in &out.textures_delta.set {
        for delta in deltas {
            let egui::ImageData::Color(image) = &delta.image;
            let key = format!("{id:?}");
            if let Some(offset) = delta.pos {
                let (target_size, target) = textures.get_mut(&key).unwrap();
                for y in 0..image.size[1] {
                    for x in 0..image.size[0] {
                        target[(offset[1] + y) * target_size[0] + offset[0] + x] =
                            image.pixels[y * image.size[0] + x].to_array();
                    }
                }
            } else {
                textures.insert(
                    key,
                    (
                        image.size,
                        image.pixels.iter().map(|c| c.to_array()).collect(),
                    ),
                );
            }
        }
    }
    out.textures_delta.clear();
}

/// Every piece of text drawn in a frame, with its position.
pub fn texts(out: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    fn collect(shape: &egui::Shape, into: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, into)),
            egui::Shape::Text(t) => into.push((t.galley.job.text.clone(), t.pos)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    out.shapes.iter().for_each(|s| collect(&s.shape, &mut all));
    all
}

fn press(pos: egui::Pos2) -> Vec<egui::Event> {
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![egui::Event::PointerMoved(pos), button(true), button(false)]
}

/// Runs `draw` for a few frames (plus three per step) and writes `target/ui-review/{name}.*`.
pub fn export(
    name: &str,
    size: [f32; 2],
    scale: f32,
    tokens: Tokens,
    steps: &[Step],
    draw: impl FnMut(&mut egui::Ui),
) {
    let ctx = egui::Context::default();
    super::text::install_fonts(&ctx);
    tokens::install(&ctx, tokens);
    export_in(&ctx, name, size, scale, steps, draw);
}

/// Like `export`, for a context the caller has already set up (fonts, colours, textures).
pub fn export_in(
    ctx: &egui::Context,
    name: &str,
    size: [f32; 2],
    scale: f32,
    steps: &[Step],
    mut draw: impl FnMut(&mut egui::Ui),
) {
    ctx.set_zoom_factor(scale);
    let output_dir = PathBuf::from("target/ui-review");
    std::fs::create_dir_all(&output_dir).unwrap();
    let mut textures = Atlas::new();
    let mut geometry = Vec::new();
    let mut last_text: Vec<(String, egui::Pos2)> = Vec::new();
    let mut clock = 0.0;
    let mut hover = None;
    let input = |clock: f64, events: Vec<egui::Event>| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::from(size) / scale,
        )),
        time: Some(clock),
        events,
        ..Default::default()
    };
    for frame in 0..3 + steps.len() * 3 {
        let mut events = Vec::new();
        if frame >= 3 && (frame - 3) % 3 == 0 {
            match &steps[(frame - 3) / 3] {
                Step::Click(label) => {
                    let at = last_text
                        .iter()
                        .rfind(|(text, _)| text == label)
                        .unwrap_or_else(|| panic!("Missing {label}"))
                        .1;
                    events = press(at + egui::vec2(4.0, 5.0));
                }
                Step::ClickAt(x, y) => events = press(egui::pos2(*x, *y)),
                Step::Hover(x, y) => {
                    let pos = egui::pos2(*x, *y);
                    hover = Some(pos);
                    events = vec![egui::Event::PointerMoved(pos)];
                }
            }
        }
        clock += if frame >= 3 { 0.25 } else { 0.05 };
        let mut out = ctx.run_ui(input(clock, events), |root| draw(root));
        if hover.is_some() {
            // Tooltips wait for a still pointer, so let time pass without moving it.
            for _ in 0..4 {
                apply_textures(&mut textures, &mut out);
                clock += 0.3;
                out = ctx.run_ui(input(clock, vec![]), |root| draw(root));
            }
        }
        last_text = texts(&out);
        apply_textures(&mut textures, &mut out);
        let primitives = ctx.tessellate(out.shapes.clone(), out.pixels_per_point);
        let ppp = out.pixels_per_point;
        geometry = primitives
            .into_iter()
            .filter_map(|p| match p.primitive {
                egui::epaint::Primitive::Mesh(mesh) => Some(serde_json::json!({
                    "clip": [p.clip_rect.min.x * ppp, p.clip_rect.min.y * ppp, p.clip_rect.max.x * ppp, p.clip_rect.max.y * ppp],
                    "texture": format!("{:?}", mesh.texture_id),
                    "vertices": mesh.vertices.iter().map(|v| (v.pos.x * ppp, v.pos.y * ppp, v.uv.x, v.uv.y, v.color.to_array())).collect::<Vec<_>>(),
                    "indices": mesh.indices,
                })),
                _ => None,
            })
            .collect();
    }
    let texture_json: Vec<_> = textures
        .into_iter()
        .enumerate()
        .map(|(n, (id, (texture_size, pixels)))| {
            let file = format!("{name}-atlas-{n}.rgba");
            std::fs::write(
                output_dir.join(&file),
                pixels.into_iter().flatten().collect::<Vec<_>>(),
            )
            .unwrap();
            serde_json::json!({"id": id, "size": texture_size, "file": file})
        })
        .collect();
    std::fs::write(
        output_dir.join(format!("{name}.json")),
        serde_json::to_vec(&serde_json::json!({
            "size": size,
            "background": ctx.global_style().visuals.panel_fill.to_array(),
            "textures": texture_json,
            "meshes": geometry,
        }))
        .unwrap(),
    )
    .unwrap();
}
