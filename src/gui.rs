use std::{
  path::PathBuf,
  process::Command,
  sync::mpsc::{self, Receiver, TryRecvError},
  thread,
  time::Duration,
};

use eframe::egui;
use mhrise_save_converter::conversion::{
  ConversionProgress, ConversionRequest, PreflightReport, TargetPlatform,
  convert_path_with_progress, preflight_path,
};

enum WorkerEvent {
  Progress(ConversionProgress),
  Finished(Result<Vec<PathBuf>, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuiTarget {
  Steam,
  Switch,
}

impl GuiTarget {
  fn platform(self) -> TargetPlatform {
    match self {
      Self::Steam => TargetPlatform::Steam,
      Self::Switch => TargetPlatform::NintendoSwitch,
    }
  }
}

pub struct GuiApp {
  source: String,
  output: String,
  target_reference: String,
  source_steamid64: String,
  target_steamid64: String,
  source_curve_index: String,
  target_curve_index: String,
  target: GuiTarget,
  overwrite: bool,
  preflight: Option<PreflightReport>,
  status: String,
  progress: Option<ConversionProgress>,
  output_directory: Option<PathBuf>,
  worker: Option<Receiver<WorkerEvent>>,
}

impl Default for GuiApp {
  fn default() -> Self {
    Self {
      source: String::new(),
      output: String::new(),
      target_reference: String::new(),
      source_steamid64: String::new(),
      target_steamid64: String::new(),
      source_curve_index: String::new(),
      target_curve_index: String::new(),
      target: GuiTarget::Steam,
      overwrite: false,
      preflight: None,
      status: "Choose a source save directory to begin.".to_owned(),
      progress: None,
      output_directory: None,
      worker: None,
    }
  }
}

impl GuiApp {
  fn request(&self) -> Result<ConversionRequest, String> {
    let parse_id = |value: &str, label: &str| {
      if value.trim().is_empty() {
        Ok(None)
      } else {
        value.trim().parse::<u64>().map(Some).map_err(|_| format!("{label} must be a number"))
      }
    };
    let parse_curve = |value: &str, label: &str| {
      if value.trim().is_empty() {
        Ok(None)
      } else {
        value.trim().parse::<usize>().map(Some).map_err(|_| format!("{label} must be a number"))
      }
    };

    Ok(ConversionRequest {
      target: self.target.platform(),
      source_steamid64: parse_id(&self.source_steamid64, "Source SteamID64")?,
      target_steamid64: parse_id(&self.target_steamid64, "Target SteamID64")?,
      source_curve_index: parse_curve(&self.source_curve_index, "Source Curve Index")?,
      target_curve_index: parse_curve(&self.target_curve_index, "Target Curve Index")?,
      target_reference: (!self.target_reference.trim().is_empty())
        .then(|| PathBuf::from(self.target_reference.trim())),
      force: self.overwrite,
    })
  }

  fn paths(&self) -> Result<(PathBuf, PathBuf), String> {
    if self.source.trim().is_empty() {
      return Err("Choose a source save directory first.".to_owned());
    }
    if self.output.trim().is_empty() {
      return Err("Choose an output directory first.".to_owned());
    }
    Ok((PathBuf::from(self.source.trim()), PathBuf::from(self.output.trim())))
  }

  fn choose_folder(value: &mut String) {
    if let Some(path) = rfd::FileDialog::new().pick_folder() {
      *value = path.display().to_string();
    }
  }

  fn run_preflight(&mut self) {
    let result = (|| {
      let (source, output) = self.paths()?;
      let request = self.request()?;
      preflight_path(&source, &output, &request).map_err(|error| error.to_string())
    })();
    match result {
      Ok(report) => {
        self.status = if report.can_convert() {
          "Preflight passed. Ready to convert.".to_owned()
        } else {
          "Preflight found errors. Fix them before converting.".to_owned()
        };
        self.preflight = Some(report);
      }
      Err(error) => {
        self.preflight = None;
        self.status = error;
      }
    }
  }

  fn start_conversion(&mut self) {
    let prepared = (|| {
      let (source, output) = self.paths()?;
      let request = self.request()?;
      let report = preflight_path(&source, &output, &request).map_err(|error| error.to_string())?;
      if !report.can_convert() {
        let message = report.errors().collect::<Vec<_>>().join("; ");
        return Err(message);
      }
      Ok((source, output, request, report))
    })();

    let Ok((source, output, request, report)) = prepared else {
      self.run_preflight();
      return;
    };
    let (sender, receiver) = mpsc::channel();
    self.preflight = Some(report);
    self.progress = None;
    self.output_directory = Some(output.clone());
    self.status = "Converting…".to_owned();
    self.worker = Some(receiver);
    thread::spawn(move || {
      let result = convert_path_with_progress(&source, &output, request, |progress| {
        let _ = sender.send(WorkerEvent::Progress(progress));
      })
      .map_err(|error| error.to_string());
      let _ = sender.send(WorkerEvent::Finished(result));
    });
  }

  fn poll_worker(&mut self) {
    let Some(receiver) = self.worker.take() else {
      return;
    };
    let mut keep_receiver = true;
    loop {
      match receiver.try_recv() {
        Ok(WorkerEvent::Progress(progress)) => self.progress = Some(progress),
        Ok(WorkerEvent::Finished(result)) => {
          keep_receiver = false;
          match result {
            Ok(files) => self.status = format!("Finished: {} file(s) written.", files.len()),
            Err(error) => self.status = format!("Conversion failed: {error}"),
          }
        }
        Err(TryRecvError::Empty) => break,
        Err(TryRecvError::Disconnected) => {
          keep_receiver = false;
          self.status = "Conversion worker stopped unexpectedly.".to_owned();
          break;
        }
      }
    }
    if keep_receiver {
      self.worker = Some(receiver);
    }
  }

  fn open_output_directory(&mut self) {
    let Some(path) = self.output_directory.as_deref() else {
      self.status = "No converted output is available yet.".to_owned();
      return;
    };
    let command = if cfg!(target_os = "macos") {
      "open"
    } else if cfg!(target_os = "windows") {
      "explorer"
    } else {
      "xdg-open"
    };
    match Command::new(command).arg(path).spawn() {
      Ok(_) => self.status = format!("Opened {}", path.display()),
      Err(error) => self.status = format!("Could not open output directory: {error}"),
    }
  }

  fn render_path_row(ui: &mut egui::Ui, label: &str, value: &mut String, browse: bool) {
    ui.horizontal(|ui| {
      ui.label(label);
      ui.add(egui::TextEdit::singleline(value).desired_width(480.0));
      if browse && ui.button("Browse…").clicked() {
        Self::choose_folder(value);
      }
    });
  }
}

impl eframe::App for GuiApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    self.poll_worker();
    egui::CentralPanel::default().show(ctx, |ui| {
      ui.heading("Monster Hunter Rise Save Converter");
      ui.label("Convert a complete save directory while keeping the original untouched.");
      ui.separator();

      Self::render_path_row(ui, "Source", &mut self.source, true);
      Self::render_path_row(ui, "Output", &mut self.output, true);
      Self::render_path_row(ui, "Template", &mut self.target_reference, true);

      ui.horizontal(|ui| {
        ui.label("Target");
        ui.selectable_value(&mut self.target, GuiTarget::Steam, "Steam");
        ui.selectable_value(&mut self.target, GuiTarget::Switch, "Nintendo Switch");
      });

      ui.collapsing("Steam and advanced options", |ui| {
        ui.horizontal(|ui| {
          ui.label("Source SteamID64");
          ui.text_edit_singleline(&mut self.source_steamid64);
        });
        ui.horizontal(|ui| {
          ui.label("Target SteamID64");
          ui.text_edit_singleline(&mut self.target_steamid64);
        });
        ui.horizontal(|ui| {
          ui.label("Source Curve Index");
          ui.text_edit_singleline(&mut self.source_curve_index);
        });
        ui.horizontal(|ui| {
          ui.label("Target Curve Index");
          ui.text_edit_singleline(&mut self.target_curve_index);
        });
        ui.checkbox(&mut self.overwrite, "Allow writing to a non-empty output directory");
      });

      ui.horizontal(|ui| {
        let running = self.worker.is_some();
        if ui.add_enabled(!running, egui::Button::new("Check save")).clicked() {
          self.run_preflight();
        }
        if ui.add_enabled(!running, egui::Button::new("Convert")).clicked() {
          self.start_conversion();
        }
      });

      ui.separator();
      ui.label(&self.status);
      ui.horizontal(|ui| {
        let output_available = self.output_directory.as_deref().is_some_and(|path| path.is_dir());
        if ui.add_enabled(output_available, egui::Button::new("Open output folder")).clicked() {
          self.open_output_directory();
        }
      });
      if let Some(progress) = &self.progress {
        let fraction = progress.completed as f32 / progress.total.max(1) as f32;
        ui.add(
          egui::ProgressBar::new(fraction)
            .text(format!("{} / {}", progress.completed, progress.total)),
        );
        ui.label(format!("Current: {}", progress.current_file.display()));
      }
      if let Some(report) = &self.preflight {
        ui.separator();
        ui.label(format!(
          "Files: {} ({} core, {} album/photo)",
          report.file_count, report.core_file_count, report.auxiliary_file_count
        ));
        for error in report.errors() {
          ui.colored_label(egui::Color32::RED, format!("Error: {error}"));
        }
        for warning in report.warnings() {
          ui.colored_label(egui::Color32::YELLOW, format!("Warning: {warning}"));
        }
      }
    });
    if self.worker.is_some() {
      ctx.request_repaint_after(Duration::from_millis(100));
    }
  }
}
