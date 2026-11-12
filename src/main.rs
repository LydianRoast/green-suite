// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
mod analysis;
mod app;
mod appearance;
mod connection;
mod headless;
mod live;
mod measurement;
mod model;
mod pdf;
mod standards;
mod traverse;
mod units;
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
    if !args.iter().any(|arg| arg == "--help" || arg == "-h")
        && let Some(path) = option("--replay-usb")
    {
        let report = usb::replay::inspect(std::path::Path::new(&path))?;
        let json = serde_json::to_string_pretty(&report)?;
        if let Some(path) = option("--report") {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .context("Creating replay report without replacing an existing file")?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
        }
        println!("{json}");
        anyhow::ensure!(
            report.transactions > 0,
            "Capture contains no stored USB transactions"
        );
        anyhow::ensure!(
            report.rejected_transactions == 0 && report.invalid_lines == 0,
            "Replay found rejected transactions or invalid capture lines; inspect the report"
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--usb-inventory")
        && !args.iter().any(|a| a == "--help" || a == "-h")
    {
        let mut tracker = usb::inventory::Tracker::default();
        tracker.update(usb::inventory::enumerate()?)?;
        let report = tracker.redacted_summary(false, false);
        if let Some(path) = option("--report") {
            usb::inventory::write_summary(std::path::Path::new(&path), &report)?;
        }
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Green Suite\n\nRun without arguments for the native GUI.\n  --project PATH           Open a saved project\n  --probe COM5 [--out DIR] Read USB device information; optionally save diagnostics\n  --download COM5 --out DIR Download stored readings and raw packets\n  --capture-live COM5 --seconds N --out DIR\n                           Record a bounded live USB session\n  --pdf-check PATH          Inspect and render a PDF's first page\n  --demo                   Open the clearly labeled example project\n  --view traverse|readings|live\n                           Select initial view\n  --screenshot PATH        Save a GUI screenshot and exit\n  --preview-ui PATH        Render a windowless UI preview\n  --size WIDTHxHEIGHT      Preview dimensions (default 1280x800)\n  --theme dark|light       Preview appearance\n  --palette evergreen|urbane\n                           Preview color scheme"
        );
        println!(
            "  --replay-usb PATH [--report FILE]\n                           Inspect a stored USB capture offline; never opens a port"
        );
        println!(
            "  --usb-inventory [--report FILE]\n                           Redacted OS port inventory; never opens a serial handle"
        );
        return Ok(());
    }
    if let Some(output) = option("--preview-ui") {
        let size = option("--size").unwrap_or_else(|| "1280x800".into());
        let (width, height) = size
            .split_once('x')
            .context("--size requires WIDTHxHEIGHT")?;
        let width: u32 = width.parse().context("Invalid preview width")?;
        let height: u32 = height.parse().context("Invalid preview height")?;
        anyhow::ensure!(
            (1000..=3840).contains(&width) && (600..=2160).contains(&height),
            "Preview size must be 1000–3840 by 600–2160 pixels"
        );
        let ctx = eframe::egui::Context::default();
        let mut app = app::GreenSuite::preview(
            &ctx,
            &option("--view").unwrap_or_default(),
            option("--theme").as_deref() == Some("dark"),
            appearance::Scheme::from_key(&option("--palette").unwrap_or_default()),
            args.iter().any(|arg| arg == "--demo"),
        );
        headless::render_png(&ctx, width, height, &PathBuf::from(&output), |ctx| {
            app.ui(ctx)
        })?;
        println!(
            "{}",
            serde_json::json!({"preview":output,"width":width,"height":height,"headless":true})
        );
        return Ok(());
    }
    if let Some(port) = option("--probe") {
        let d = if let Some(out) = option("--out") {
            let out = PathBuf::from(out);
            std::fs::create_dir_all(&out)?;
            let path = out.join(format!("probe-{}.raw.ndjson", uuid::Uuid::new_v4()));
            eprintln!("Identification capture: {}", path.display());
            usb::probe_logged(&port, &path)?
        } else {
            usb::probe(&port)?
        };
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
    if let Some(port) = option("--capture-live") {
        use std::cell::{Cell, RefCell};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let seconds: u64 = option("--seconds")
            .context("--capture-live requires --seconds (1 to 3600)")?
            .parse()
            .context("--seconds must be an integer")?;
        anyhow::ensure!(
            (1..=3600).contains(&seconds),
            "--seconds must be between 1 and 3600"
        );
        let out =
            PathBuf::from(option("--out").context("--capture-live requires --out DIRECTORY")?);
        let device = usb::probe(&port)?;
        anyhow::ensure!(
            device.firmware.trim() == "Rev. 1.04Y6",
            "Live USB is currently validated only for Rev. 1.04Y6"
        );
        std::fs::create_dir_all(&out)?;
        let raw_path = out.join(format!("live-{}.raw.ndjson", uuid::Uuid::new_v4()));
        let stop = Arc::new(AtomicBool::new(false));
        let deadline_stop = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            deadline_stop.store(true, Ordering::Release);
        });
        let samples = Cell::new(0usize);
        let latest = RefCell::new(None);
        let warnings = RefCell::new(Vec::new());
        let mode_changed = Cell::new(false);
        let result = live::monitor(&port, &raw_path, stop, |event| match event {
            live::LiveEvent::Sample(sample) => {
                samples.set(samples.get() + 1);
                *latest.borrow_mut() = Some(sample);
            }
            live::LiveEvent::Warning(warning) => warnings.borrow_mut().push(warning),
            live::LiveEvent::SessionChanged => mode_changed.set(true),
            live::LiveEvent::Modules(_) => {}
        })
        .and_then(|()| {
            anyhow::ensure!(
                samples.get() > 0,
                "No validated live measurements arrived; the raw capture was retained"
            );
            Ok(())
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "samples": samples.get(), "latest":latest.into_inner(), "warnings":warnings.into_inner(),
                "raw_capture":raw_path, "session_changed":mode_changed.get(),
                "device_mode_restored":false, "error":result.as_ref().err().map(|e| format!("{e:#}"))
            }))?
        );
        if mode_changed.get() {
            eprintln!(
                "Reception ended. To return to normal use, turn sensors off, restart the Reporter, then turn sensors on."
            );
        }
        return result;
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
