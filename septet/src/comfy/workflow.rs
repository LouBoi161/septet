//! The graphs Septet queues: Qwen Image 2.1 text-to-image and edit by instruction (the API
//! format, built here so every input Septet fills is known instead of guessed from node numbers).
//! Generative Fill is an edit of the area around the selection; the app keeps the result only
//! inside the selection (`docs/suite/PLAN.md`, Phase H).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Which Qwen Image 2.1 model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Model {
    /// Distilled for a fixed 8-step schedule: about 35 s for 2 MP on a 12 GB card.
    #[default]
    Turbo,
    /// The full model, 40 steps: slower, more detail.
    Base,
}

impl Model {
    pub const ALL: [Model; 2] = [Model::Turbo, Model::Base];

    pub fn label(self) -> &'static str {
        match self {
            Model::Turbo => "Fast (Turbo, 8 steps)",
            Model::Base => "Detailed (Base, 40 steps)",
        }
    }

    fn unet(self) -> &'static str {
        match self {
            Model::Turbo => "qwen_image_2.1_turbo_int8_convrot.safetensors",
            Model::Base => "qwen_image_2.1_int8_convrot.safetensors",
        }
    }
}

const CLIP: &str = "qwen3vl_8b_int8_convrot.safetensors";
const VAE: &str = "qwen_image_2.1_vae_bf16.safetensors";
/// Turbo is distilled for exactly this schedule (the official template).
const TURBO_SIGMAS: &str = "1.0, 0.978453, 0.95418, 0.926626, 0.89508, 0.845148, 0.704534, 0.414568, 0";
const BASE_STEPS: u32 = 40;
/// The node that hands over the result; Septet reads only its images.
pub const SAVE_NODE: &str = "8";

/// Sizes the model takes: multiples of 32 within these bounds.
pub const MIN_SIDE: u32 = 256;
pub const MAX_SIDE: u32 = 2816;

/// A side snapped to what the model takes.
pub fn snap(side: u32) -> u32 {
    (side.clamp(MIN_SIDE, MAX_SIDE) + 16) / 32 * 32
}

/// The size to generate for a `w` × `h` target: its aspect ratio at about `pixels` pixels
/// (never below ~1.5 MP for Turbo's sake, never above the model's limit), in multiples of 32.
pub fn size_for(w: u32, h: u32, pixels: f64) -> (u32, u32) {
    let (w, h) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let scale = (pixels / (w * h)).sqrt();
    let (mut sw, mut sh) = (w * scale, h * scale);
    let over = (sw.max(sh) / f64::from(MAX_SIDE)).max(1.0);
    sw /= over;
    sh /= over;
    (snap(sw.round() as u32), snap(sh.round() as u32))
}

fn base(model: Model, prompt: &str, seed: u64, resolution: u32) -> Value {
    let mut g = json!({
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": model.unet(), "weight_dtype": "default"}},
        "2": {"class_type": "CLIPLoader", "inputs": {"clip_name": CLIP, "type": "qwen_image", "device": "default"}},
        "3": {"class_type": "VAELoader", "inputs": {"vae_name": VAE}},
        "4": {"class_type": "TextEncodeQwenImage21", "inputs": {"clip": ["2", 0], "prompt": prompt, "negative_prompt": "", "resolution": resolution}},
        // The text and reference tokens are computed once, not at every step.
        "11": {"class_type": "QwenImage21Cache", "inputs": {"model": ["1", 0], "device": "auto", "dtype": "default"}},
        "12": {"class_type": "RandomNoise", "inputs": {"noise_seed": seed}},
        "13": {"class_type": "KSamplerSelect", "inputs": {"sampler_name": "euler"}},
        "15": {"class_type": "CFGGuider", "inputs": {"model": ["11", 0], "positive": ["4", 0], "negative": ["4", 1], "cfg": 1.0}},
        "6": {"class_type": "SamplerCustomAdvanced", "inputs": {"noise": ["12", 0], "guider": ["15", 0], "sampler": ["13", 0], "sigmas": ["14", 0], "latent_image": ["5", 0]}},
        "7": {"class_type": "VAEDecode", "inputs": {"samples": ["6", 0], "vae": ["3", 0]}},
        // A preview, not a save: the result goes to ComfyUI's temp folder, not the user's outputs.
        SAVE_NODE: {"class_type": "PreviewImage", "inputs": {"images": ["7", 0]}},
    });
    g["14"] = match model {
        Model::Turbo => json!({"class_type": "ManualSigmas", "inputs": {"sigmas": TURBO_SIGMAS}}),
        Model::Base => json!({"class_type": "BasicScheduler", "inputs": {"model": ["1", 0], "scheduler": "simple", "steps": BASE_STEPS, "denoise": 1.0}}),
    };
    g
}

/// Text to image at `width` × `height` (snapped).
pub fn generate(model: Model, prompt: &str, seed: u64, width: u32, height: u32) -> Value {
    let mut g = base(model, prompt, seed, 1024);
    g["5"] = json!({"class_type": "EmptyLatentImage", "inputs": {"width": snap(width), "height": snap(height), "batch_size": 1}});
    g
}

/// Edit `images` (names `upload_png` returned; the first is the one edited, `<image1>` in the
/// prompt) by the instruction `prompt`. The result has image 1's aspect at about
/// `resolution`² pixels.
pub fn edit(model: Model, prompt: &str, seed: u64, images: &[String], resolution: u32) -> Value {
    let mut g = base(model, prompt, seed, resolution);
    g["4"]["inputs"]["vae"] = json!(["3", 0]);
    for (i, name) in images.iter().enumerate().take(10) {
        let node = format!("10{i}");
        g[&node] = json!({"class_type": "LoadImage", "inputs": {"image": name}});
        g["4"]["inputs"][format!("images.image_{}", i + 1)] = json!([node, 0]);
    }
    // The encoder's empty latent has image 1's size: sampling at any other size shifts the edit.
    g["6"]["inputs"]["latent_image"] = json!(["4", 2]);
    g
}

/// The instruction for Generative Fill: the area to fill is flat grey in the image (the app
/// keeps the result only inside the selection, so what the model does elsewhere is lost anyway).
pub fn fill_prompt(what: &str) -> String {
    let what = what.trim();
    if what.is_empty() {
        "Fill the flat grey area in <image1> with what would naturally be behind it, continuing the surroundings seamlessly, as if nothing had \
         been there. Keep everything outside the grey area exactly as it is."
            .into()
    } else {
        format!(
            "In <image1>, fill the flat grey area with: {what}. Match the surroundings' perspective, lighting, colours and grain so it blends \
             in seamlessly. Keep everything outside the grey area exactly as it is."
        )
    }
}

/// The grey Generative Fill paints over the selection before the edit.
pub const FILL_GREY: [u8; 3] = [128, 128, 128];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_keep_the_aspect_within_the_models_limits() {
        assert_eq!(snap(1000), 992);
        assert_eq!(snap(10), MIN_SIDE);
        assert_eq!(snap(9999), MAX_SIDE);
        let (w, h) = size_for(1920, 1080, 2.36e6);
        assert_eq!((w % 32, h % 32), (0, 0));
        assert!(((w as f64 / h as f64) - 16.0 / 9.0).abs() < 0.05, "{w}×{h}");
        let (w, h) = size_for(10_000, 100, 2e6);
        assert_eq!(w, MAX_SIDE);
        assert!(h >= MIN_SIDE);
    }

    #[test]
    fn graphs_wire_the_inputs() {
        let g = generate(Model::Turbo, "a cat", 7, 1000, 600);
        assert_eq!(g["5"]["inputs"]["width"], 992);
        assert_eq!(g["12"]["inputs"]["noise_seed"], 7);
        assert_eq!(g["14"]["class_type"], "ManualSigmas");
        let g = edit(Model::Base, "make it red", 1, &["a.png".into(), "b.png".into()], 1024);
        assert_eq!(g["4"]["inputs"]["images.image_2"], json!(["101", 0]));
        assert_eq!(g["101"]["inputs"]["image"], "b.png");
        assert_eq!(g["6"]["inputs"]["latent_image"], json!(["4", 2]));
        assert_eq!(g["14"]["inputs"]["steps"], BASE_STEPS);
        assert!(g.get("5").is_none_or(Value::is_null));
        assert!(fill_prompt(" a boat ").contains("fill the flat grey area with: a boat."));
    }
}
