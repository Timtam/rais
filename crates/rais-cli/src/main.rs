use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use rais_core::artifact::{
    ArtifactDescriptor, CachedArtifact, default_cache_dir, download_artifacts,
    resolve_latest_artifacts,
};
use rais_core::detection::{DiscoveryOptions, detect_components, discover_installations};
use rais_core::install::{InstallOptions, InstallReport, install_cached_artifacts};
use rais_core::latest::fetch_latest_versions;
use rais_core::localization::{DEFAULT_LOCALE, LocalizedText, Localizer, available_locales};
use rais_core::model::{Architecture, Platform};
use rais_core::operation::{
    PackageOperationOptions, PackageOperationReport, execute_package_operation,
};
use rais_core::package::{
    builtin_package_specs, default_desired_package_ids, embedded_package_manifest,
};
use rais_core::plan::{AvailablePackage, build_install_plan};
use rais_core::portable::{PortabilityCheckStatus, PortabilityReport, check_portable_runtime};
use rais_core::preflight::{PreflightOptions, PreflightReport, run_install_preflight};
use rais_core::report::{default_report_path, save_json_and_text_reports};
use rais_core::resource::{ResourceInitActionKind, ResourceInitReport, initialize_resource_path};
use rais_core::rollback::{
    BackupSet, RestoreBackupActionKind, RestoreBackupOptions, RestoreBackupReport,
    list_backup_sets, restore_backup_set,
};
use rais_core::setup::{SetupOptions, SetupReport, execute_setup_operation};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "rais")]
#[command(about = "Diagnostic CLI for REAPER Accessibility Installation Software")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Detect {
        #[arg(long)]
        portable: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Components {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Latest {
        #[arg(long)]
        json: bool,
    },
    Artifacts {
        #[arg(long)]
        package: Vec<String>,
        #[arg(long, value_enum)]
        architecture: Option<CliArchitecture>,
        #[arg(long)]
        json: bool,
    },
    Download {
        #[arg(long)]
        package: Vec<String>,
        #[arg(long, value_enum)]
        architecture: Option<CliArchitecture>,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Packages {
        #[arg(long)]
        manifest: bool,
        #[arg(long)]
        json: bool,
    },
    Preflight {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        target_app_path: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        allow_reaper_running: bool,
        #[arg(long)]
        report_path: Option<PathBuf>,
        #[arg(long)]
        save_report: bool,
        #[arg(long)]
        json: bool,
    },
    InitResource {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        target_app_path: Option<PathBuf>,
        #[arg(long)]
        portable: bool,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        allow_reaper_running: bool,
        #[arg(long)]
        report_path: Option<PathBuf>,
        #[arg(long)]
        save_report: bool,
        #[arg(long)]
        json: bool,
    },
    Backups {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    RestoreBackup {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        backup_id: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        allow_reaper_running: bool,
        #[arg(long)]
        report_path: Option<PathBuf>,
        #[arg(long)]
        save_report: bool,
        #[arg(long)]
        json: bool,
    },
    InstallExtension {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        target_app_path: Option<PathBuf>,
        #[arg(long, required = true)]
        package: Vec<String>,
        #[arg(long, value_enum)]
        architecture: Option<CliArchitecture>,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        allow_reaper_running: bool,
        #[arg(long)]
        report_path: Option<PathBuf>,
        #[arg(long)]
        save_report: bool,
        #[arg(long)]
        json: bool,
    },
    ApplyPackages {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        target_app_path: Option<PathBuf>,
        #[arg(long)]
        package: Vec<String>,
        #[arg(long, value_enum)]
        architecture: Option<CliArchitecture>,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        allow_reaper_running: bool,
        #[arg(long)]
        stage_unsupported: bool,
        #[arg(long)]
        replace_osara_keymap: bool,
        #[arg(long)]
        report_path: Option<PathBuf>,
        #[arg(long)]
        save_report: bool,
        #[arg(long)]
        json: bool,
    },
    Setup {
        #[arg(long)]
        resource_path: PathBuf,
        #[arg(long)]
        target_app_path: Option<PathBuf>,
        #[arg(long)]
        portable: bool,
        #[arg(long)]
        package: Vec<String>,
        #[arg(long, value_enum)]
        architecture: Option<CliArchitecture>,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        allow_reaper_running: bool,
        #[arg(long)]
        stage_unsupported: bool,
        #[arg(long)]
        replace_osara_keymap: bool,
        #[arg(long)]
        report_path: Option<PathBuf>,
        #[arg(long)]
        save_report: bool,
        #[arg(long)]
        json: bool,
    },
    Locales {
        #[arg(long, default_value = "locales")]
        locales_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Localize {
        #[arg(long, default_value_t = DEFAULT_LOCALE.to_string())]
        locale: String,
        #[arg(long, default_value = "locales")]
        locales_dir: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long = "arg")]
        args: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    PortableCheck {
        #[arg(long, default_value = "locales")]
        locales_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Plan {
        #[arg(long)]
        resource_path: Option<PathBuf>,
        #[arg(long)]
        portable: Vec<PathBuf>,
        #[arg(long)]
        online: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
        #[arg(long)]
        report_path: Option<PathBuf>,
        #[arg(long)]
        save_report: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliArchitecture {
    X86,
    X64,
    Arm64,
    Arm64Ec,
    Universal,
}

impl From<CliArchitecture> for Architecture {
    fn from(value: CliArchitecture) -> Self {
        match value {
            CliArchitecture::X86 => Self::X86,
            CliArchitecture::X64 => Self::X64,
            CliArchitecture::Arm64 => Self::Arm64,
            CliArchitecture::Arm64Ec => Self::Arm64Ec,
            CliArchitecture::Universal => Self::Universal,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Detect { portable, json } => {
            let installations = discover_installations(&DiscoveryOptions {
                include_standard: true,
                portable_roots: portable,
            })?;

            if json {
                println!("{}", serde_json::to_string_pretty(&installations)?);
            } else {
                print_installations(&installations);
            }
        }
        Command::Components {
            resource_path,
            json,
        } => {
            let platform = Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
            let components = detect_components(&resource_path, platform)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&components)?);
            } else {
                print_components(&components);
            }
        }
        Command::Latest { json } => {
            let latest = fetch_latest_versions()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&latest)?);
            } else {
                print_latest(&latest);
            }
        }
        Command::Artifacts {
            package,
            architecture,
            json,
        } => {
            let platform = Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
            let architecture = architecture.map_or_else(Architecture::current, Into::into);
            let packages = selected_package_ids(package);
            let artifacts = resolve_latest_artifacts(&packages, platform, architecture)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&artifacts)?);
            } else {
                print_artifacts(&artifacts);
            }
        }
        Command::Download {
            package,
            architecture,
            cache_dir,
            json,
        } => {
            let platform = Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
            let architecture = architecture.map_or_else(Architecture::current, Into::into);
            let packages = selected_package_ids(package);
            let artifacts = resolve_latest_artifacts(&packages, platform, architecture)?;
            let cache_dir = cache_dir.unwrap_or_else(default_cache_dir);
            let cached = download_artifacts(&artifacts, &cache_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&cached)?);
            } else {
                print_cached_artifacts(&cached);
            }
        }
        Command::Packages { manifest, json } => {
            if manifest {
                let manifest = embedded_package_manifest();
                if json {
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                } else {
                    println!("Schema version: {}", manifest.schema_version);
                    for package in &manifest.packages {
                        println!("{}", package.id);
                        println!("  Display name: {}", package.display_name);
                        println!("  Kind: {}", serialized_name(&package.package_kind));
                        println!("  Required: {}", yes_no(package.required));
                        println!("  Recommended: {}", yes_no(package.recommended));
                        println!(
                            "  Supported platforms: {}",
                            serialized_names(&package.supported_platforms)
                        );
                        println!(
                            "  Supported architectures: {}",
                            serialized_names(&package.supported_architectures)
                        );
                        println!(
                            "  Latest provider: {}",
                            optional_serialized_name(package.latest_version_provider.as_ref())
                        );
                        println!(
                            "  Artifact provider: {}",
                            optional_serialized_name(package.artifact_provider.as_ref())
                        );
                        println!("  Detectors: {}", serialized_names(&package.detectors));
                        println!(
                            "  Install steps: {}",
                            serialized_names(&package.install_steps)
                        );
                        println!(
                            "  Uninstall steps: {}",
                            serialized_names(&package.uninstall_steps)
                        );
                        println!(
                            "  Backup policy: {}",
                            serialized_name(&package.backup_policy)
                        );
                        println!(
                            "  Windows suffixes: {}",
                            string_names(&package.user_plugin_suffixes.windows)
                        );
                        println!(
                            "  macOS suffixes: {}",
                            string_names(&package.user_plugin_suffixes.macos)
                        );
                    }
                }
            } else {
                let platform =
                    Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
                let packages = builtin_package_specs(platform);
                if json {
                    println!("{}", serde_json::to_string_pretty(&packages)?);
                } else {
                    print_package_specs(&packages);
                }
            }
        }
        Command::Preflight {
            resource_path,
            target_app_path,
            dry_run,
            allow_reaper_running,
            report_path,
            save_report,
            json,
        } => {
            let report = run_install_preflight(
                &resource_path,
                &PreflightOptions {
                    dry_run,
                    allow_reaper_running,
                    target_app_path,
                },
            );
            let report_path =
                selected_report_path(Some(&resource_path), report_path, save_report, "preflight")?;
            save_optional_report(report_path.as_deref(), &report)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_preflight_report(&report);
            }
            if !report.passed {
                std::process::exit(2);
            }
        }
        Command::InitResource {
            resource_path,
            target_app_path,
            portable,
            apply,
            allow_reaper_running,
            report_path,
            save_report,
            json,
        } => {
            let report = initialize_resource_path(
                &resource_path,
                &rais_core::resource::ResourceInitOptions {
                    dry_run: !apply,
                    portable,
                    allow_reaper_running,
                    target_app_path,
                },
            )?;
            let report_path = selected_report_path(
                Some(&resource_path),
                report_path,
                save_report,
                "init-resource",
            )?;
            save_optional_report(report_path.as_deref(), &report)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_resource_init_report(&report);
            }
        }
        Command::Backups {
            resource_path,
            json,
        } => {
            let backup_sets = list_backup_sets(&resource_path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&backup_sets)?);
            } else {
                print_backup_sets(&backup_sets);
            }
        }
        Command::RestoreBackup {
            resource_path,
            backup_id,
            apply,
            allow_reaper_running,
            report_path,
            save_report,
            json,
        } => {
            let report = restore_backup_set(
                &resource_path,
                &backup_id,
                &RestoreBackupOptions {
                    dry_run: !apply,
                    allow_reaper_running,
                },
            )?;
            let report_path = selected_report_path(
                Some(&resource_path),
                report_path,
                save_report,
                "restore-backup",
            )?;
            save_optional_report(report_path.as_deref(), &report)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_restore_backup_report(&report);
            }
        }
        Command::InstallExtension {
            resource_path,
            target_app_path,
            package,
            architecture,
            cache_dir,
            apply,
            allow_reaper_running,
            report_path,
            save_report,
            json,
        } => {
            let platform = Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
            let architecture = architecture.map_or_else(Architecture::current, Into::into);
            let artifacts = resolve_latest_artifacts(&package, platform, architecture)?;
            let cache_dir = cache_dir.unwrap_or_else(default_cache_dir);
            let cached = download_artifacts(&artifacts, &cache_dir)?;
            let report = install_cached_artifacts(
                &resource_path,
                &cached,
                &InstallOptions {
                    dry_run: !apply,
                    allow_reaper_running,
                    target_app_path,
                },
            )?;
            let report_path = selected_report_path(
                Some(&resource_path),
                report_path,
                save_report,
                "install-extension",
            )?;
            save_optional_report(report_path.as_deref(), &report)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_install_report(&report);
            }
        }
        Command::ApplyPackages {
            resource_path,
            target_app_path,
            package,
            architecture,
            cache_dir,
            apply,
            allow_reaper_running,
            stage_unsupported,
            replace_osara_keymap,
            report_path,
            save_report,
            json,
        } => {
            let platform = Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
            let architecture = architecture.map_or_else(Architecture::current, Into::into);
            let packages = selected_package_ids(package);
            let cache_dir = cache_dir.unwrap_or_else(default_cache_dir);
            let report = execute_package_operation(
                &resource_path,
                &packages,
                platform,
                architecture,
                &cache_dir,
                &PackageOperationOptions {
                    dry_run: !apply,
                    allow_reaper_running,
                    stage_unsupported,
                    replace_osara_keymap,
                    target_app_path,
                },
            )?;
            let report_path = selected_report_path(
                Some(&resource_path),
                report_path,
                save_report,
                "apply-packages",
            )?;
            save_optional_report(report_path.as_deref(), &report)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_package_operation_report(&report);
            }
        }
        Command::Setup {
            resource_path,
            target_app_path,
            portable,
            package,
            architecture,
            cache_dir,
            apply,
            allow_reaper_running,
            stage_unsupported,
            replace_osara_keymap,
            report_path,
            save_report,
            json,
        } => {
            let platform = Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
            let architecture = architecture.map_or_else(Architecture::current, Into::into);
            let packages = selected_package_ids(package);
            let cache_dir = cache_dir.unwrap_or_else(default_cache_dir);
            let report = execute_setup_operation(
                &resource_path,
                &packages,
                platform,
                architecture,
                &cache_dir,
                &SetupOptions {
                    dry_run: !apply,
                    portable,
                    allow_reaper_running,
                    stage_unsupported,
                    replace_osara_keymap,
                    target_app_path,
                },
            )?;
            let report_path =
                selected_report_path(Some(&resource_path), report_path, save_report, "setup")?;
            save_optional_report(report_path.as_deref(), &report)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_setup_report(&report);
            }
        }
        Command::Locales { locales_dir, json } => {
            let locales = available_locales(&locales_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&locales)?);
            } else {
                print_locales(&locales);
            }
        }
        Command::Localize {
            locale,
            locales_dir,
            id,
            args,
            json,
        } => {
            let localizer = Localizer::from_locale_dir(&locales_dir, &locale)?;
            let owned_args = parse_localization_args(args)?;
            let borrowed_args = owned_args
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            let message = localizer.format(&id, &borrowed_args);
            if json {
                println!("{}", serde_json::to_string_pretty(&message)?);
            } else {
                print_localized_text(&message);
            }
        }
        Command::PortableCheck { locales_dir, json } => {
            let report = check_portable_runtime(&locales_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_portability_report(&report);
            }
        }
        Command::Plan {
            resource_path,
            portable,
            online,
            format,
            report_path,
            save_report,
        } => {
            let platform = Platform::current().ok_or(rais_core::RaisError::UnsupportedPlatform)?;
            let installations = discover_installations(&DiscoveryOptions {
                include_standard: true,
                portable_roots: portable,
            })?;
            let explicit_resource_path = resource_path.clone();
            let target = match resource_path.as_ref() {
                Some(path) => installations
                    .iter()
                    .find(|installation| installation.resource_path == *path)
                    .cloned(),
                None => installations.first().cloned(),
            };
            let plan_report_resource_path = resource_path.clone().or_else(|| {
                target
                    .as_ref()
                    .map(|installation| installation.resource_path.clone())
            });
            let detection_path = explicit_resource_path.or_else(|| {
                target
                    .as_ref()
                    .map(|installation| installation.resource_path.clone())
            });
            let components = match detection_path {
                Some(path) => detect_components(&path, platform)?,
                None => Vec::new(),
            };

            let desired = default_desired_package_ids();
            let available = if online {
                fetch_latest_versions()?
            } else {
                Vec::new()
            };
            let plan = build_install_plan(target, &components, &desired, &available);
            let report_path = selected_report_path(
                plan_report_resource_path.as_deref(),
                report_path,
                save_report,
                "plan",
            )?;
            save_optional_report(report_path.as_deref(), &plan)?;
            match format {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&plan)?),
                OutputFormat::Text => print_plan(&plan),
            }
        }
    }

    Ok(())
}

fn parse_localization_args(args: Vec<String>) -> rais_core::Result<Vec<(String, String)>> {
    args.into_iter()
        .map(|arg| {
            let Some((name, value)) = arg.split_once('=') else {
                return Err(rais_core::RaisError::Localization {
                    path: None,
                    message: format!("localization argument must use name=value: {arg}"),
                });
            };
            if name.is_empty() {
                return Err(rais_core::RaisError::Localization {
                    path: None,
                    message: format!("localization argument name is empty: {arg}"),
                });
            }
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

fn selected_package_ids(package_ids: Vec<String>) -> Vec<String> {
    if package_ids.is_empty() {
        default_desired_package_ids()
    } else {
        package_ids
    }
}

fn save_optional_report<T>(
    report_path: Option<&Path>,
    report: &T,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: serde::Serialize + ?Sized,
{
    if let Some(report_path) = report_path {
        let saved = save_json_and_text_reports(report_path, report)?;
        eprintln!("Report saved (JSON): {}", saved.json_path.display());
        eprintln!("Report saved (text): {}", saved.text_path.display());
    }
    Ok(())
}

fn selected_report_path(
    resource_path: Option<&Path>,
    explicit_report_path: Option<PathBuf>,
    save_report: bool,
    operation_name: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    if let Some(path) = explicit_report_path {
        return Ok(Some(path));
    }

    if save_report {
        let Some(resource_path) = resource_path else {
            let error = std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--save-report requires a selected resource path",
            );
            return Err(Box::new(error));
        };
        return Ok(Some(default_report_path(resource_path, operation_name)));
    }

    Ok(None)
}

fn print_installations(installations: &[rais_core::model::Installation]) {
    if installations.is_empty() {
        println!("No REAPER installations detected.");
        return;
    }

    for (index, installation) in installations.iter().enumerate() {
        println!("Installation {}", index + 1);
        println!("  Type: {:?}", installation.kind);
        println!("  App: {}", installation.app_path.display());
        println!("  Resource path: {}", installation.resource_path.display());
        println!(
            "  Version: {}",
            installation
                .version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "  Architecture: {}",
            installation
                .architecture
                .as_ref()
                .map(|architecture| format!("{architecture:?}"))
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!("  Writable: {}", yes_no(installation.writable));
        println!("  Confidence: {:?}", installation.confidence);
        println!();
    }
}

fn print_components(components: &[rais_core::model::ComponentDetection]) {
    for component in components {
        println!("{}", component.display_name);
        println!("  Installed: {}", yes_no(component.installed));
        println!(
            "  Version: {}",
            component
                .version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!("  Detector: {}", component.detector);
        if !component.files.is_empty() {
            println!("  Files:");
            for file in &component.files {
                println!("    {}", file.display());
            }
        }
        for note in &component.notes {
            println!("  Note: {note}");
        }
        println!();
    }
}

fn print_latest(latest: &[AvailablePackage]) {
    for package in latest {
        println!(
            "{}: {}",
            package.package_id,
            package
                .version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string())
        );
    }
}

fn print_artifacts(artifacts: &[ArtifactDescriptor]) {
    for artifact in artifacts {
        println!("{}", artifact.package_id);
        println!("  Version: {}", artifact.version);
        println!("  Platform: {:?}", artifact.platform);
        println!("  Architecture: {:?}", artifact.architecture);
        println!("  Kind: {:?}", artifact.kind);
        println!("  File: {}", artifact.file_name);
        println!("  URL: {}", artifact.url);
    }
}

fn print_cached_artifacts(cached: &[CachedArtifact]) {
    for artifact in cached {
        println!("{}", artifact.descriptor.package_id);
        println!("  Version: {}", artifact.descriptor.version);
        println!("  File: {}", artifact.path.display());
        println!("  Size: {}", artifact.size);
        println!("  SHA-256: {}", artifact.sha256);
        println!(
            "  Reused existing file: {}",
            yes_no(artifact.reused_existing_file)
        );
    }
}

fn print_install_report(report: &InstallReport) {
    println!("Resource path: {}", report.resource_path.display());
    println!("Dry run: {}", yes_no(report.dry_run));
    print_preflight_report(&report.preflight);
    println!("Receipt written: {}", yes_no(report.receipt_written));
    if let Some(receipt_backup_path) = &report.receipt_backup_path {
        println!("Receipt backup: {}", receipt_backup_path.display());
    }
    if let Some(backup_manifest_path) = &report.backup_manifest_path {
        println!("Backup manifest: {}", backup_manifest_path.display());
    }
    for action in &report.actions {
        println!("{}", action.package_id);
        println!("  Action: {:?}", action.action);
        println!("  Source: {}", action.source_path.display());
        println!("  Target: {}", action.target_path.display());
        if let Some(backup_path) = &action.backup_path {
            println!("  Backup: {}", backup_path.display());
        }
        println!("  Size: {}", action.size);
        println!("  SHA-256: {}", action.sha256);
    }
}

fn print_resource_init_report(report: &ResourceInitReport) {
    println!("Resource path: {}", report.resource_path.display());
    println!("Dry run: {}", yes_no(report.dry_run));
    println!("Portable layout: {}", yes_no(report.portable));
    print_preflight_report(&report.preflight);
    for action in &report.actions {
        let verb = match action.action {
            ResourceInitActionKind::WouldCreate => "Would create",
            ResourceInitActionKind::Created => "Created",
            ResourceInitActionKind::AlreadyExists => "Already exists",
        };
        println!("  {verb} {:?}: {}", action.kind, action.path.display());
    }
}

fn print_backup_sets(backup_sets: &[BackupSet]) {
    if backup_sets.is_empty() {
        println!("No backup sets found.");
        return;
    }

    for backup_set in backup_sets {
        println!("{}", backup_set.id);
        println!("  Path: {}", backup_set.path.display());
        if let Some(created_at) = &backup_set.created_at {
            println!("  Created: {created_at}");
        }
        if let Some(reason) = &backup_set.reason {
            println!("  Reason: {reason}");
        }
        if let Some(manifest_path) = &backup_set.manifest_path {
            println!("  Manifest: {}", manifest_path.display());
        }
        println!("  Files: {}", backup_set.files.len());
        for file in &backup_set.files {
            println!("    {}", file.display());
        }
    }
}

fn print_restore_backup_report(report: &RestoreBackupReport) {
    println!("Resource path: {}", report.resource_path.display());
    println!("Backup id: {}", report.backup_id);
    println!("Backup path: {}", report.backup_path.display());
    println!("Dry run: {}", yes_no(report.dry_run));
    print_preflight_report(&report.preflight);
    for action in &report.actions {
        let verb = match action.action {
            RestoreBackupActionKind::WouldRestore => "Would restore",
            RestoreBackupActionKind::Restored => "Restored",
        };
        println!("  {verb}: {}", action.target_path.display());
        println!("    Source: {}", action.source_path.display());
        if let Some(current_backup_path) = &action.current_backup_path {
            println!("    Current file backup: {}", current_backup_path.display());
        }
        println!("    Size: {}", action.size);
        println!("    SHA-256: {}", action.sha256);
    }
}

fn print_package_operation_report(report: &PackageOperationReport) {
    println!("Resource path: {}", report.resource_path.display());
    println!("Dry run: {}", yes_no(report.dry_run));
    if let Some(install_report) = &report.install_report {
        print_preflight_report(&install_report.preflight);
    }

    for item in &report.items {
        println!("{}", item.package_id);
        println!("  Plan action: {:?}", item.plan_action);
        println!("  Status: {:?}", item.status);
        println!("  Kind: {:?}", item.artifact.kind);
        println!("  Version: {}", item.artifact.version);
        println!("  URL: {}", item.artifact.url);
        println!("  Message: {}", item.message);
        if let Some(instruction) = &item.manual_instruction {
            println!("  Manual step: {}", instruction.title);
            for step in &instruction.steps {
                println!("    Step: {step}");
            }
            for note in &instruction.notes {
                println!("    Note: {note}");
            }
        }
        if let Some(cached) = &item.cached_artifact {
            println!("  Cached: {}", cached.path.display());
            println!("  SHA-256: {}", cached.sha256);
        }
        if let Some(action) = &item.install_action {
            println!("  Install action: {:?}", action.action);
            println!("  Target: {}", action.target_path.display());
        }
    }
}

fn print_setup_report(report: &SetupReport) {
    println!("Setup resource path: {}", report.resource_path.display());
    println!("Dry run: {}", yes_no(report.dry_run));
    println!();
    println!("Resource initialization");
    print_resource_init_report(&report.resource_init);
    println!();
    println!("Package operation");
    print_package_operation_report(&report.package_operation);
}

fn print_package_specs(packages: &[rais_core::package::PackageSpec]) {
    for package in packages {
        println!("{}", package.id);
        println!("  Display name: {}", package.display_name);
        println!("  Display name key: {}", package.display_name_key);
        println!("  Kind: {}", serialized_name(&package.package_kind));
        println!("  Required: {}", yes_no(package.required));
        println!("  Recommended: {}", yes_no(package.recommended));
        println!(
            "  Supported platforms: {}",
            serialized_names(&package.supported_platforms)
        );
        println!(
            "  Supported architectures: {}",
            serialized_names(&package.supported_architectures)
        );
        println!(
            "  Latest provider: {}",
            optional_serialized_name(package.latest_version_provider.as_ref())
        );
        println!(
            "  Artifact provider: {}",
            optional_serialized_name(package.artifact_provider.as_ref())
        );
        println!("  Detectors: {}", serialized_names(&package.detectors));
        println!(
            "  Install steps: {}",
            serialized_names(&package.install_steps)
        );
        println!(
            "  Uninstall steps: {}",
            serialized_names(&package.uninstall_steps)
        );
        println!(
            "  Backup policy: {}",
            serialized_name(&package.backup_policy)
        );
        println!(
            "  Plugin prefixes: {}",
            string_names(&package.user_plugin_prefixes)
        );
        println!(
            "  Plugin suffixes: {}",
            string_names(&package.user_plugin_suffixes)
        );
    }
}

fn serialized_name<T: Serialize + ?Sized>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        Ok(value) => value.to_string(),
        Err(_) => "(invalid)".to_string(),
    }
}

fn optional_serialized_name<T: Serialize + ?Sized>(value: Option<&T>) -> String {
    value
        .map(serialized_name)
        .unwrap_or_else(|| "(none)".to_string())
}

fn serialized_names<T: Serialize>(values: &[T]) -> String {
    let names = values.iter().map(serialized_name).collect::<Vec<_>>();
    string_names(&names)
}

fn string_names(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_string()
    } else {
        values.join(", ")
    }
}

fn print_preflight_report(report: &PreflightReport) {
    println!("Preflight passed: {}", yes_no(report.passed));
    for check in &report.checks {
        println!("  {}: {:?}: {}", check.name, check.status, check.message);
    }
}

fn print_plan(plan: &rais_core::plan::InstallPlan) {
    if let Some(target) = &plan.target {
        println!("Target resource path: {}", target.resource_path.display());
    } else {
        println!("Target resource path: not selected");
    }

    for action in &plan.actions {
        println!("{}", action.package_id);
        println!("  Action: {:?}", action.action);
        println!(
            "  Installed version: {}",
            action
                .installed_version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!(
            "  Available version: {}",
            action
                .available_version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string())
        );
        println!("  Reason: {}", action.reason);
    }

    for note in &plan.notes {
        println!("Note: {note}");
    }
}

fn print_locales(locales: &[String]) {
    for locale in locales {
        println!("{locale}");
    }
}

fn print_localized_text(message: &LocalizedText) {
    println!("{}", message.value);
    println!("  Id: {}", message.id);
    println!("  Locale: {}", message.locale);
    println!("  Fallback: {}", yes_no(message.fallback_used));
    println!("  Missing: {}", yes_no(message.missing));
    for error in &message.formatting_errors {
        println!("  Formatting error: {error}");
    }
}

fn print_portability_report(report: &PortabilityReport) {
    println!("Portable runtime passed: {}", yes_no(report.passed));
    println!(
        "Executable: {}",
        report
            .current_exe
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!("Current directory: {}", report.current_dir.display());
    println!(
        "Locales directory: {} ({})",
        report.locales_dir.display(),
        if report.locales_dir_present {
            "present"
        } else {
            "absent"
        }
    );
    println!("Embedded resources: {}", report.embedded_resources.len());
    for resource in &report.embedded_resources {
        println!(
            "  {} {} ({} bytes)",
            resource.kind, resource.id, resource.bytes
        );
    }
    println!(
        "Required external resources: {}",
        report.required_external_resources.len()
    );
    for check in &report.checks {
        println!(
            "  {}: {}: {}",
            check.name,
            portability_status_label(check.status),
            check.message
        );
    }
}

fn portability_status_label(status: PortabilityCheckStatus) -> &'static str {
    match status {
        PortabilityCheckStatus::Passed => "passed",
        PortabilityCheckStatus::Warning => "warning",
        PortabilityCheckStatus::Failed => "failed",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn setup_command_parses_target_app_path() {
        let cli = Cli::try_parse_from([
            "rais",
            "setup",
            "--resource-path",
            "C:\\PortableREAPER",
            "--target-app-path",
            "C:\\PortableREAPER\\reaper.exe",
            "--portable",
        ])
        .unwrap();

        match cli.command {
            Command::Setup {
                resource_path,
                target_app_path,
                portable,
                ..
            } => {
                assert_eq!(resource_path, PathBuf::from("C:\\PortableREAPER"));
                assert_eq!(
                    target_app_path,
                    Some(PathBuf::from("C:\\PortableREAPER\\reaper.exe"))
                );
                assert!(portable);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn preflight_command_parses_target_app_path() {
        let cli = Cli::try_parse_from([
            "rais",
            "preflight",
            "--resource-path",
            "C:\\Users\\Test\\AppData\\Roaming\\REAPER",
            "--target-app-path",
            "C:\\Program Files\\REAPER\\reaper.exe",
        ])
        .unwrap();

        match cli.command {
            Command::Preflight {
                target_app_path, ..
            } => {
                assert_eq!(
                    target_app_path,
                    Some(PathBuf::from("C:\\Program Files\\REAPER\\reaper.exe"))
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
