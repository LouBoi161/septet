//! `image_generate` and `image_edit`: Claude makes and edits pictures with the user's local
//! ComfyUI (Qwen Image 2.1), in the same queue as the apps' Generative Fill (`comfy`).

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{Value, json};

use super::mcp::ToolReply;
use crate::comfy::{Handle, Job, Model, Task, workflow};

pub fn definitions() -> Vec<Value> {
    let quality =
        json!({"type": "string", "enum": ["fast", "detailed"], "description": "fast (default, ~35 s for 2 MP) or detailed (40 steps, several times slower)."});
    let seed = json!({"type": "integer", "description": "Same seed and prompt give the same image; omit for a new one."});
    vec![
        json!({
            "name": "image_generate",
            "description": "Make a picture from a description with Qwen Image 2.1 on the user's computer (ComfyUI; it may take a minute or two the first time) and save it as PNG in the workspace. Write the prompt like the caption of the finished image: one English paragraph with medium and style, the subject, where things are, colours, materials, the light. Text that should appear in the image goes in \"straight quotes\". Then place or open the file with septet_place / septet_open.",
            "inputSchema": {"type": "object", "properties": {
                "prompt": {"type": "string"},
                "width": {"type": "integer", "description": "256–2816, rounded to a multiple of 32 (default 2048)."},
                "height": {"type": "integer", "description": "256–2816, rounded to a multiple of 32 (default 1152). Stay above ~1.5 megapixels."},
                "quality": quality,
                "seed": seed,
                "path": {"type": "string", "description": "Where to save the PNG, in the workspace."},
            }, "required": ["prompt", "path"]},
        }),
        json!({
            "name": "image_edit",
            "description": "Edit a picture by instruction with Qwen Image 2.1 on the user's computer and save the result as PNG in the workspace. `images[0]` is the picture that is edited (<image1> in the prompt); up to 9 more are references (<image2>, …). Say what stays the same and what changes, e.g. \"Keep the person in <image1> unchanged, replace the background with a beach at noon.\"",
            "inputSchema": {"type": "object", "properties": {
                "images": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 10, "description": "PNG/JPEG files in the workspace or opened by the user."},
                "prompt": {"type": "string"},
                "resolution": {"type": "integer", "description": "The result's size, about this many pixels squared (512–2048, default 1024), with image 1's aspect."},
                "quality": quality,
                "seed": seed,
                "path": {"type": "string", "description": "Where to save the PNG, in the workspace."},
            }, "required": ["images", "prompt", "path"]},
        }),
    ]
}

pub fn is_image_tool(name: &str) -> bool {
    matches!(name, "image_generate" | "image_edit")
}

/// Run on a worker thread: `inputs` are the edit's images, already checked to be allowed.
pub fn run(comfy: &Handle, workspace: &Path, name: &str, args: &Value, inputs: &[PathBuf]) -> Result<ToolReply, String> {
    let prompt = args["prompt"].as_str().filter(|p| !p.trim().is_empty()).ok_or("`prompt` is required.")?.to_owned();
    let out = super::assets::in_workspace(workspace, args["path"].as_str().ok_or("`path` is required.")?)?;
    let model = if args["quality"] == "detailed" { Model::Base } else { Model::Turbo };
    let seed = args["seed"].as_u64().unwrap_or_else(|| uuid::Uuid::new_v4().as_u64_pair().0 >> 12);
    let task = if name == "image_edit" {
        let images = inputs.iter().map(|p| image::open(p).map(|i| i.to_rgba8()).map_err(|e| format!("{}: {e}", p.display()))).collect::<Result<Vec<_>, _>>()?;
        let resolution = workflow::snap(args["resolution"].as_u64().unwrap_or(1024).clamp(512, 2048) as u32);
        Task::Edit { prompt, images, resolution }
    } else {
        let w = args["width"].as_u64().unwrap_or(2048).clamp(256, 2816) as u32;
        let h = args["height"].as_u64().unwrap_or(1152).clamp(256, 2816) as u32;
        Task::Generate { prompt, width: w, height: h }
    };
    let img = comfy.run(Job { task, model, seed })?;
    img.save(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    // Claude sees a smaller copy; the file keeps the full size.
    let preview = image::imageops::thumbnail(&img, 768.min(img.width()), (768 * img.height() / img.width().max(1)).min(img.height()).max(1));
    let mut png = Vec::new();
    preview.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(ToolReply {
        content: vec![
            json!({"type": "text", "text": format!("Saved {} ({} × {} px, seed {seed}).", out.display(), img.width(), img.height())}),
            json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png), "mimeType": "image/png"}),
        ],
        is_error: false,
    })
}
