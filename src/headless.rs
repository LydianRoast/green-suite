// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.

//! CPU-only egui previews. This module creates no native windows, GPU contexts,
//! input hooks, or device connections. It renders the same tessellated UI meshes
//! used by the desktop interface; GPU-specific callbacks are rejected explicitly.

use anyhow::{Context, Result, bail, ensure};
use eframe::egui::{
    self, Color32, Context as EguiContext, Pos2, Rect, TextureFilter, TextureId, TextureOptions,
    TextureWrapMode,
    epaint::{ClippedPrimitive, ImageData, ImageDelta, Mesh, Primitive, Vertex},
    pos2, vec2,
};
use image::{Rgba, RgbaImage};
use std::{collections::HashMap, path::Path};

/// Paint a stable preview at one physical pixel per UI point. The caller must
/// provide an isolated in-memory app state without device or save operations.
pub fn render_png(
    ctx: &EguiContext,
    width: u32,
    height: u32,
    path: &Path,
    mut draw: impl FnMut(&EguiContext),
) -> Result<()> {
    ensure!(
        (128..=4096).contains(&width) && (128..=4096).contains(&height),
        "Headless preview dimensions must be between 128 and 4096 pixels"
    );
    ctx.set_zoom_factor(1.0);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32));
    let mut textures = HashMap::new();
    let mut final_primitives = Vec::new();
    // Egui needs an initial sizing pass for panels, grids, and scroll areas.
    // Eight passes settle layout, scrolling and font-atlas updates without waits.
    for frame in 0..8 {
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            max_texture_side: Some(4096),
            // Deterministic virtual time lets window fade-in animations finish
            // before the final pass, without sleeping or opening a window.
            time: Some(f64::from(frame) / 4.0),
            ..Default::default()
        };
        if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.native_pixels_per_point = Some(1.0);
            viewport.inner_rect = Some(screen);
            viewport.minimized = Some(false);
            viewport.focused = Some(true);
        }
        let output = ctx.run(input, |context| draw(context));
        for (id, delta) in output.textures_delta.set {
            update_texture(&mut textures, id, delta)?;
        }
        if frame == 7 {
            final_primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        } else {
            for id in output.textures_delta.free {
                textures.remove(&id);
            }
        }
    }
    let background = ctx.style().visuals.panel_fill;
    let mut image = RgbaImage::from_pixel(width, height, Rgba(background.to_array()));
    paint(&mut image, &textures, &final_primitives)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).context("Creating headless preview directory")?;
    }
    image.save(path).context("Saving headless UI preview")
}

struct Texture {
    size: [usize; 2],
    pixels: Vec<Color32>,
    options: TextureOptions,
}

fn update_texture(
    textures: &mut HashMap<TextureId, Texture>,
    id: TextureId,
    delta: ImageDelta,
) -> Result<()> {
    let ImageData::Color(image) = delta.image;
    ensure!(
        image.size[0] > 0
            && image.size[1] > 0
            && image.size[0] <= 8192
            && image.size[1] <= 8192
            && image.pixels.len() == image.size[0] * image.size[1],
        "Unsupported headless texture dimensions"
    );
    if let Some([left, top]) = delta.pos {
        let texture = textures
            .get_mut(&id)
            .context("Partial update references a missing headless texture")?;
        ensure!(
            left.checked_add(image.size[0])
                .is_some_and(|right| right <= texture.size[0])
                && top
                    .checked_add(image.size[1])
                    .is_some_and(|bottom| bottom <= texture.size[1]),
            "Headless texture update lies outside its image"
        );
        for row in 0..image.size[1] {
            let target = (top + row) * texture.size[0] + left;
            let source = row * image.size[0];
            texture.pixels[target..target + image.size[0]]
                .copy_from_slice(&image.pixels[source..source + image.size[0]]);
        }
        texture.options = delta.options;
    } else {
        textures.insert(
            id,
            Texture {
                size: image.size,
                pixels: image.pixels.clone(),
                options: delta.options,
            },
        );
    }
    Ok(())
}

fn paint(
    image: &mut RgbaImage,
    textures: &HashMap<TextureId, Texture>,
    primitives: &[ClippedPrimitive],
) -> Result<()> {
    let bounds = Rect::from_min_size(
        Pos2::ZERO,
        vec2(image.width() as f32, image.height() as f32),
    );
    for clipped in primitives {
        let clip = clipped.clip_rect.intersect(bounds);
        if !clip.is_positive() {
            continue;
        }
        let mesh = match &clipped.primitive {
            Primitive::Mesh(mesh) => mesh,
            Primitive::Callback(_) => {
                bail!("GPU callbacks cannot be rendered in a headless preview")
            }
        };
        ensure!(mesh.is_valid(), "Invalid headless UI mesh");
        let texture = textures
            .get(&mesh.texture_id)
            .context("UI mesh references a missing headless texture")?;
        ensure!(
            texture.options.wrap_mode == TextureWrapMode::ClampToEdge,
            "Repeating textures are not supported by the headless UI renderer"
        );
        paint_mesh(image, texture, mesh, clip);
    }
    Ok(())
}

fn edge(a: Pos2, b: Pos2, point: Pos2) -> f32 {
    (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x)
}

fn inclusive_edge(a: Pos2, b: Pos2) -> bool {
    b.y < a.y || (b.y == a.y && b.x > a.x)
}

fn paint_mesh(image: &mut RgbaImage, texture: &Texture, mesh: &Mesh, clip: Rect) {
    for triangle in mesh.indices.chunks_exact(3) {
        let mut vertices = [
            mesh.vertices[triangle[0] as usize],
            mesh.vertices[triangle[1] as usize],
            mesh.vertices[triangle[2] as usize],
        ];
        let mut area = edge(vertices[0].pos, vertices[1].pos, vertices[2].pos);
        if !area.is_finite() || area.abs() <= f32::EPSILON {
            continue;
        }
        if area < 0.0 {
            vertices.swap(1, 2);
            area = -area;
        }
        let left = vertices
            .iter()
            .map(|v| v.pos.x)
            .fold(f32::INFINITY, f32::min)
            .max(clip.min.x)
            .floor()
            .max(0.0) as u32;
        let right = vertices
            .iter()
            .map(|v| v.pos.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .min(clip.max.x)
            .ceil()
            .min(image.width() as f32) as u32;
        let top = vertices
            .iter()
            .map(|v| v.pos.y)
            .fold(f32::INFINITY, f32::min)
            .max(clip.min.y)
            .floor()
            .max(0.0) as u32;
        let bottom = vertices
            .iter()
            .map(|v| v.pos.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .min(clip.max.y)
            .ceil()
            .min(image.height() as f32) as u32;
        for y in top..bottom {
            for x in left..right {
                let position = pos2(x as f32 + 0.5, y as f32 + 0.5);
                if position.x < clip.min.x
                    || position.x >= clip.max.x
                    || position.y < clip.min.y
                    || position.y >= clip.max.y
                {
                    continue;
                }
                let edges = [
                    edge(vertices[1].pos, vertices[2].pos, position),
                    edge(vertices[2].pos, vertices[0].pos, position),
                    edge(vertices[0].pos, vertices[1].pos, position),
                ];
                if edges.iter().enumerate().any(|(index, value)| {
                    *value < 0.0
                        || (*value == 0.0
                            && !inclusive_edge(
                                vertices[(index + 1) % 3].pos,
                                vertices[(index + 2) % 3].pos,
                            ))
                }) {
                    continue;
                }
                let weights = edges.map(|value| value / area);
                let uv = interpolate_uv(vertices, weights);
                let texel = sample_texture(texture, uv);
                let mut source = [0.0f32; 4];
                for (channel, component) in source.iter_mut().enumerate() {
                    let tint: f32 = vertices
                        .iter()
                        .zip(weights)
                        .map(|(vertex, weight)| f32::from(vertex.color[channel]) * weight)
                        .sum();
                    *component = texel[channel] * tint / 255.0;
                }
                let target = image.get_pixel_mut(x, y);
                let remainder = 1.0 - source[3] / 255.0;
                for (channel, component) in target.0.iter_mut().enumerate() {
                    *component = (source[channel] + f32::from(*component) * remainder)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

fn interpolate_uv(vertices: [Vertex; 3], weights: [f32; 3]) -> Pos2 {
    pos2(
        vertices.iter().zip(weights).map(|(v, w)| v.uv.x * w).sum(),
        vertices.iter().zip(weights).map(|(v, w)| v.uv.y * w).sum(),
    )
}

fn sample_texture(texture: &Texture, uv: Pos2) -> [f32; 4] {
    let x = uv.x.clamp(0.0, 1.0) * texture.size[0] as f32 - 0.5;
    let y = uv.y.clamp(0.0, 1.0) * texture.size[1] as f32 - 0.5;
    let pixel = |x: f32, y: f32| {
        let x = x.clamp(0.0, texture.size[0] as f32 - 1.0) as usize;
        let y = y.clamp(0.0, texture.size[1] as f32 - 1.0) as usize;
        texture.pixels[y * texture.size[0] + x]
            .to_array()
            .map(f32::from)
    };
    if texture.options.magnification == TextureFilter::Nearest {
        return pixel(x.round(), y.round());
    }
    let left = x.floor();
    let top = y.floor();
    let dx = x - left;
    let dy = y - top;
    let samples = [
        pixel(left, top),
        pixel(left + 1.0, top),
        pixel(left, top + 1.0),
        pixel(left + 1.0, top + 1.0),
    ];
    let mut color = [0.0; 4];
    for (channel, value) in color.iter_mut().enumerate() {
        *value = samples[0][channel] * (1.0 - dx) * (1.0 - dy)
            + samples[1][channel] * dx * (1.0 - dy)
            + samples[2][channel] * (1.0 - dx) * dy
            + samples[3][channel] * dx * dy;
    }
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_translucent_quad_does_not_blend_shared_diagonal_twice() {
        let texture = Texture {
            size: [1, 1],
            pixels: vec![Color32::WHITE],
            options: TextureOptions::LINEAR,
        };
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 4.0)),
            Color32::from_rgba_unmultiplied(255, 0, 0, 128),
        );
        let mut image = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        paint_mesh(
            &mut image,
            &texture,
            &mesh,
            Rect::from_min_max(pos2(1.0, 1.0), pos2(3.0, 3.0)),
        );
        let red = Color32::from_rgba_unmultiplied(255, 0, 0, 128).r();
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    image.get_pixel(x, y).0,
                    if (1..3).contains(&x) && (1..3).contains(&y) {
                        [red, 0, 0, 255]
                    } else {
                        [0, 0, 0, 255]
                    }
                );
            }
        }
    }

    #[test]
    fn font_atlas_updates_preserve_untouched_texels() {
        let mut textures = HashMap::new();
        let id = TextureId::Managed(0);
        update_texture(
            &mut textures,
            id,
            ImageDelta::full(
                egui::ColorImage::filled([3, 2], Color32::BLACK),
                TextureOptions::LINEAR,
            ),
        )
        .unwrap();
        update_texture(
            &mut textures,
            id,
            ImageDelta::partial(
                [1, 1],
                egui::ColorImage::filled([2, 1], Color32::WHITE),
                TextureOptions::LINEAR,
            ),
        )
        .unwrap();
        assert_eq!(
            textures[&id].pixels,
            vec![
                Color32::BLACK,
                Color32::BLACK,
                Color32::BLACK,
                Color32::BLACK,
                Color32::WHITE,
                Color32::WHITE
            ]
        );
    }
}
