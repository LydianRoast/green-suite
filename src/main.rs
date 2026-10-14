// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
mod app;
mod model;
mod pdf;
mod traverse;
mod usb;

use anyhow::{Context, Result};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let option = |key: &str| {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Green Suite\n\nRun without arguments for the native GUI.\n  --project PATH           Open a saved project\n  --probe COM5             Read USB device information\n  --download COM5 --out DIR Download stored readings and raw packets\n  --pdf-check PATH          Inspect and render a PDF's first page\n  --demo                   Open the clearly labeled example project\n  --view traverse|readings  Select initial view\n  --screenshot PATH        Save a GUI screenshot and exit"
        );
        return Ok(());
    }
    if let Some(port) = option("--probe") {
        let d = usb::probe(&port)?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"port":d.port,"model":d.model,"serial":d.serial,"firmware":d.firmware,"groups":d.group_count,"reportedRecords":d.record_count})
            )?
        );
        return Ok(());
    }
    if let Some(port) = option("--download") {
        let out = PathBuf::from(option("--out").context("--download requires --out DIRECTORY")?);
        std::fs::create_dir_all(&out)?;
        let capture = usb::download(&port, |s| eprintln!("{s}"))?;
        let mut project = model::Project::new(&format!("WR-401 {}", capture.device.serial));
        let n = project.merge_readings(capture.readings)?;
        let stamp = format!(
            "{}-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S"),
            uuid::Uuid::new_v4()
        );
        let serial_file: String = capture
            .device
            .serial
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = out.join(format!("{}-{stamp}.green-suite.json", serial_file));
        project.save(&path)?;
        project.export_csv(&out.join(format!("{}-{stamp}.csv", serial_file)))?;
        std::fs::write(
            out.join(format!("{}-{stamp}.raw.ndjson", serial_file)),
            capture.raw_log,
        )?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"project":path,"measurements":n,"complete":capture.complete,"warnings":capture.warnings,"device":capture.device.serial})
            )?
        );
        anyhow::ensure!(
            capture.complete,
            "Download is incomplete; partial measurements and raw packets were saved."
        );
        return Ok(());
    }
    if let Some(path) = option("--pdf-check") {
        let doc = pdf::PdfDocument::open(std::path::Path::new(&path))?;
        let (w, h, _) = doc.render(0, 1800)?;
        println!(
            "{}",
            serde_json::json!({"pages":doc.page_count(),"firstPageSize":doc.page_size(0)?,"renderWidth":w,"renderHeight":h})
        );
        return Ok(());
    }
    let project = option("--project").map(PathBuf::from);
    let screenshot = option("--screenshot").map(PathBuf::from);
    let interactive = screenshot.is_none();
    if interactive {
        detach_console();
    }
    let demo = args.iter().any(|a| a == "--demo");
    let view = option("--view").unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280., 650.])
            .with_min_inner_size([1000., 600.])
            .with_maximized(true)
            .with_title("Green Suite · Measurement Workspace"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Green Suite",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::GreenSuite::new(
                cc, project, demo, screenshot, &view,
            )))
        }),
    )
    .map_err(|e| {
        let message = format!("Could not start the native interface: {e}");
        if interactive {
            rfd::MessageDialog::new()
                .set_title("Green Suite could not start")
                .set_description(&message)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        anyhow::anyhow!(message)
    })
}

fn detach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn FreeConsole() -> i32;
        }
        // Only the interactive GUI detaches its own console. Diagnostic commands
        // return earlier and keep their standard output and redirected logs.
        unsafe {
            FreeConsole();
        }
    }
}
