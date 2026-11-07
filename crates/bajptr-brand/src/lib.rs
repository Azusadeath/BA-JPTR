//! One icon source for executable resources and the native window.
use std::{fs, io::Write, path::Path};
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = vec![0; (size * size * 4) as usize];
    let segment = |x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32| {
        let t = (((x - ax) * (bx - ax) + (y - ay) * (by - ay))
            / ((bx - ax).powi(2) + (by - ay).powi(2)))
        .clamp(0., 1.);
        ((x - ax - t * (bx - ax)).powi(2) + (y - ay - t * (by - ay)).powi(2)).sqrt()
    };
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.) / size as f32 * 64.;
                    let py = (y as f32 + (sy as f32 + 0.5) / 4.) / size as f32 * 64.;
                    let qx = (px - 32.).abs() - 18.;
                    let qy = (py - 32.).abs() - 18.;
                    if qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) > 11. {
                        continue;
                    }
                    let arrow = [
                        (17., 23., 46., 23.),
                        (39., 16., 46., 23.),
                        (39., 30., 46., 23.),
                        (47., 41., 18., 41.),
                        (25., 34., 18., 41.),
                        (25., 48., 18., 41.),
                    ]
                    .iter()
                    .any(|&(ax, ay, bx, by)| segment(px, py, ax, ay, bx, by) <= 2.);
                    let t = py / 64.;
                    let rgb = if arrow {
                        [255., 255., 255.]
                    } else {
                        [36. - 9. * t, 125. - 35. * t, 204. - 40. * t]
                    };
                    for i in 0..3 {
                        sum[i] += rgb[i];
                    }
                    sum[3] += 255.;
                }
            }
            let at = ((y * size + x) * 4) as usize;
            if sum[3] > 0. {
                for i in 0..3 {
                    pixels[at + i] = (sum[i] * 255. / sum[3]).round() as u8;
                }
                pixels[at + 3] = (sum[3] / 16.).round() as u8;
            }
        }
    }
    pixels
}
pub fn write_ico(path: &Path) -> std::io::Result<()> {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut blocks = Vec::new();
    for size in sizes {
        let image = rgba(size);
        let mask_stride = size.div_ceil(32) * 4;
        let mut block = Vec::new();
        for value in [
            40,
            size,
            size * 2,
            0x00200001,
            0,
            size * size * 4,
            0,
            0,
            0,
            0,
        ] {
            block.extend_from_slice(&value.to_le_bytes());
        }
        for y in (0..size).rev() {
            for x in 0..size {
                let at = ((y * size + x) * 4) as usize;
                block.extend_from_slice(&[image[at + 2], image[at + 1], image[at], image[at + 3]]);
            }
        }
        block.resize(block.len() + (mask_stride * size) as usize, 0);
        blocks.push(block);
    }
    let mut output = fs::File::create(path)?;
    output.write_all(&[0, 0, 1, 0, sizes.len() as u8, 0])?;
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, block) in sizes.into_iter().zip(&blocks) {
        output.write_all(&[size as u8, size as u8, 0, 0, 1, 0, 32, 0])?;
        output.write_all(&(block.len() as u32).to_le_bytes())?;
        output.write_all(&offset.to_le_bytes())?;
        offset += block.len() as u32;
    }
    for block in blocks {
        output.write_all(&block)?;
    }
    output.sync_all()
}
#[cfg(feature = "resource")]
pub fn embed_windows_icon() {
    println!("cargo:rerun-if-changed=../bajptr-brand/src/lib.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let ico = out.join("bajptr.ico");
    write_ico(&ico).expect("write application icon");
    let manifest = out.join("bajptr.manifest");
    fs::write(&manifest,r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings><longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware><dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness></windowsSettings></application></assembly>"#).unwrap();
    let rc = out.join("bajptr.rc");
    fs::write(
        &rc,
        format!(
            "1 ICON \"{}\"\n1 24 \"{}\"\n",
            ico.to_string_lossy().replace('\\', "/"),
            manifest.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .expect("embed executable icon and manifest");
}
