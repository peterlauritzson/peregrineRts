use bevy::prelude::*;

use bevy::window::WindowResolution;

use peregrine::game::GamePlugin;

use bevy::log::LogPlugin;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use std::fs;
use std::path::PathBuf;

fn setup_file_logging() -> String {
    // Create logs directory if it doesn't exist
    let log_dir = PathBuf::from("logs");
    if !log_dir.exists() {
        fs::create_dir_all(&log_dir).expect("Failed to create logs directory");
    }

    // Clean up old log files, keeping only the last 25
    cleanup_old_logs(&log_dir, 25);

    // Generate timestamped filename
    let now = chrono::Local::now();
    let log_filename = format!("peregrine_{}.log", now.format("%Y%m%d_%H%M%S"));
    let log_file_path = log_dir.join(&log_filename);
    let log_path_str = log_file_path.to_string_lossy().to_string();

    // Create file appender with timestamped filename
    let file_appender = RollingFileAppender::new(
        Rotation::NEVER, // Don't rotate during a single run
        &log_dir,
        &log_filename
    );

    // Create a formatting layer for the file
    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false); // No ANSI colors in file

    // Create a formatting layer for stdout (minimal)
    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_target(false);

    // Set up the subscriber with both layers
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            EnvFilter::new("wgpu=error,bevy_render=info,bevy_ecs=info,peregrine=info")
        });

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stdout_layer)
        .init();

    log_path_str
}

fn cleanup_old_logs(log_dir: &PathBuf, keep_count: usize) {
    if let Ok(entries) = fs::read_dir(log_dir) {
        let mut log_files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.starts_with("peregrine") && s.ends_with(".log"))
                    .unwrap_or(false)
            })
            .collect();

        // Sort by modified time (oldest first)
        log_files.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));

        // Delete oldest files if we exceed keep_count
        if log_files.len() > keep_count {
            for file in log_files.iter().take(log_files.len() - keep_count) {
                let _ = fs::remove_file(file.path());
            }
        }
    }
}

fn main() {
    // Set up file logging and get the log file path
    let log_file = setup_file_logging();
    
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Peregrine RTS - Logging to file                        ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Log file: {:<42} ║", log_file);
    println!("╚══════════════════════════════════════════════════════════╝");

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Peregrine RTS".into(),
                resolution: WindowResolution::new(1280, 720),
                resizable: true,
                ..default()
            }),
            ..default()
        }).build().disable::<LogPlugin>()) // Disable Bevy's default logging since we set up our own
        .add_plugins(GamePlugin)
        .run();
}

