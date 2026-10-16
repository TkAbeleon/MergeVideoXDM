mod ui;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::domain::{MergeReport, MergeRequest, ProgressSnapshot, ProgressUpdate};
use crate::errors::AppError;
use crate::{ffmpeg, merge};
use iced::{Subscription, Task, Theme};

#[derive(Debug, Clone)]
enum JobState {
    Waiting,
    Running,
    Done(MergeReport),
    Failed(AppError),
}

#[derive(Debug, Clone)]
struct Job {
    id: u64,
    source_dir: PathBuf,
    target_name: String,
    state: JobState,
    progress: Arc<Mutex<ProgressSnapshot>>,
    details_open: bool,
}

impl Job {
    fn snapshot(&self) -> ProgressSnapshot {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn record(&self, update: ProgressUpdate) {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record(update);
    }
}

#[derive(Debug, Clone)]
enum Message {
    AddFolders,
    PickOutputFolder,
    TargetNameChanged(u64, String),
    RemoveJob(u64),
    ToggleDetails(u64),
    StartQueue,
    RetryJob(u64),
    JobFinished(u64, Result<MergeReport, AppError>),
    ParallelismChanged(usize),
    IncludeIncompleteChanged(bool),
    ProgressTick,
    DismissNotice,
}

pub struct App {
    jobs: Vec<Job>,
    output_dir: PathBuf,
    parallelism: usize,
    include_incomplete: bool,
    queue_started: bool,
    next_id: u64,
    ffmpeg_status: Result<String, AppError>,
    notice: Option<String>,
    tick_nonce: u64,
}

pub fn run() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("MergeVideoXDM")
        .theme(|_: &App| None::<Theme>)
        .subscription(App::subscription)
        .window_size(iced::Size::new(1120.0, 850.0))
        .run()
}

impl App {
    fn new() -> Self {
        let output_dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Videos")
            .join("MergeVideoXDM");

        Self {
            jobs: Vec::new(),
            output_dir,
            parallelism: 2,
            include_incomplete: false,
            queue_started: false,
            next_id: 1,
            ffmpeg_status: ffmpeg::info(),
            notice: None,
            tick_nonce: 0,
        }
    }

    fn view(&self) -> iced::Element<'_, Message> {
        ui::view(self)
    }

    fn subscription(&self) -> Subscription<Message> {
        if self
            .jobs
            .iter()
            .any(|job| matches!(job.state, JobState::Running))
        {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::ProgressTick)
        } else {
            Subscription::none()
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddFolders => {
                if let Some(folders) = rfd::FileDialog::new()
                    .set_title("Sélectionner un ou plusieurs dossiers temporaires XDM")
                    .pick_folders()
                {
                    let mut added = 0;
                    let mut rejected = 0;
                    for folder in folders {
                        if self.add_folder(folder) {
                            added += 1;
                        } else {
                            rejected += 1;
                        }
                    }
                    self.notice = Some(if added == 0 && rejected > 0 {
                        "Aucun nouveau dossier ajouté : les dossiers sélectionnés sont déjà présents dans la file.".into()
                    } else if rejected > 0 {
                        format!("{added} dossier(s) ajouté(s), {rejected} doublon(s) ignoré(s).")
                    } else {
                        format!("{added} dossier(s) ajouté(s) à la file.")
                    });
                }
                Task::none()
            }
            Message::PickOutputFolder => {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Choisir le dossier de sortie")
                    .pick_folder()
                {
                    self.output_dir = folder;
                    self.notice =
                        Some(format!("Dossier de sortie : {}", self.output_dir.display()));
                }
                Task::none()
            }
            Message::TargetNameChanged(id, value) => {
                if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                    if !matches!(job.state, JobState::Running) {
                        job.target_name = value;
                    }
                }
                Task::none()
            }
            Message::RemoveJob(id) => {
                if self
                    .jobs
                    .iter()
                    .any(|job| job.id == id && matches!(job.state, JobState::Running))
                {
                    self.notice =
                        Some("Impossible de retirer une tâche pendant son traitement.".into());
                } else {
                    self.jobs.retain(|job| job.id != id);
                }
                Task::none()
            }
            Message::ToggleDetails(id) => {
                if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                    job.details_open = !job.details_open;
                }
                Task::none()
            }
            Message::StartQueue => {
                self.queue_started = true;
                if self.jobs.is_empty() {
                    self.notice = Some("Ajoute au moins un dossier XDM avant de démarrer.".into());
                    return Task::none();
                }
                for job in &mut self.jobs {
                    if matches!(job.state, JobState::Failed(_)) {
                        job.state = JobState::Waiting;
                    }
                }
                self.launch_available()
            }
            Message::RetryJob(id) => {
                self.queue_started = true;
                if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                    if matches!(job.state, JobState::Failed(_)) {
                        job.state = JobState::Waiting;
                        job.progress = Arc::new(Mutex::new(ProgressSnapshot::default()));
                    }
                }
                self.launch_available()
            }
            Message::JobFinished(id, result) => {
                if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                    match result {
                        Ok(report) => {
                            job.record(ProgressUpdate {
                                fraction: 1.0,
                                stage: "Terminé".into(),
                                message: format!("Fichier créé : {}", report.output.display()),
                            });
                            self.notice =
                                Some(format!("Assemblage terminé : {}", report.output.display()));
                            job.state = JobState::Done(report);
                        }
                        Err(error) => {
                            job.record(ProgressUpdate {
                                fraction: job.snapshot().fraction,
                                stage: "Échec".into(),
                                message: error.to_string(),
                            });
                            if let Some(hint) = &error.hint {
                                job.record(ProgressUpdate {
                                    fraction: job.snapshot().fraction,
                                    stage: "Aide".into(),
                                    message: hint.clone(),
                                });
                            }
                            self.notice = Some(format!(
                                "{} — consulte les détails de la tâche.",
                                error.kind.label()
                            ));
                            job.state = JobState::Failed(error);
                            job.details_open = true;
                        }
                    }
                }
                if self.queue_started {
                    self.launch_available()
                } else {
                    Task::none()
                }
            }
            Message::ParallelismChanged(value) => {
                self.parallelism = value.clamp(1, 4);
                if self.queue_started {
                    self.launch_available()
                } else {
                    Task::none()
                }
            }
            Message::IncludeIncompleteChanged(value) => {
                self.include_incomplete = value;
                Task::none()
            }
            Message::ProgressTick => {
                self.tick_nonce = self.tick_nonce.wrapping_add(1);
                Task::none()
            }
            Message::DismissNotice => {
                self.notice = None;
                Task::none()
            }
        }
    }

    fn add_folder(&mut self, folder: PathBuf) -> bool {
        if !folder.is_dir() {
            return false;
        }
        let normalized = folder.canonicalize().unwrap_or_else(|_| folder.clone());
        if self.jobs.iter().any(|job| {
            job.source_dir
                .canonicalize()
                .unwrap_or_else(|_| job.source_dir.clone())
                == normalized
        }) {
            return false;
        }

        let folder_name = folder
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("video");
        let base_name: String = folder_name
            .chars()
            .map(|character| {
                if character == '/' || character == '\\' || character == '\0' {
                    '_'
                } else {
                    character
                }
            })
            .collect();
        let target_name = self.unique_target_name(if base_name.is_empty() {
            "video"
        } else {
            &base_name
        });

        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(Job {
            id,
            source_dir: folder,
            target_name,
            state: JobState::Waiting,
            progress: Arc::new(Mutex::new(ProgressSnapshot::default())),
            details_open: false,
        });
        true
    }

    fn unique_target_name(&self, base: &str) -> String {
        let clean_base = base.trim_end_matches(".mp4");
        let mut candidate = format!("{clean_base}.mp4");
        let mut suffix = 2usize;
        while self.jobs.iter().any(|job| job.target_name == candidate) {
            candidate = format!("{clean_base}-{suffix}.mp4");
            suffix += 1;
        }
        candidate
    }

    fn launch_available(&mut self) -> Task<Message> {
        let active = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Running))
            .count();
        let slots = self.parallelism.saturating_sub(active);
        if slots == 0 {
            return Task::none();
        }

        let waiting: Vec<u64> = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Waiting))
            .take(slots)
            .map(|job| job.id)
            .collect();

        let mut tasks = Vec::with_capacity(waiting.len());
        for id in waiting {
            let request = if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                job.state = JobState::Running;
                job.progress = Arc::new(Mutex::new(ProgressSnapshot {
                    fraction: 0.0,
                    stage: "Démarrage".into(),
                    logs: vec!["Tâche démarrée.".into()],
                }));
                Some((
                    MergeRequest {
                        source_dir: job.source_dir.clone(),
                        output_dir: self.output_dir.clone(),
                        target_name: job.target_name.clone(),
                        include_incomplete: self.include_incomplete,
                    },
                    Arc::clone(&job.progress),
                ))
            } else {
                None
            };

            let Some((request, progress)) = request else {
                continue;
            };
            tasks.push(Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        merge::merge(request, move |update| {
                            progress
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .record(update);
                        })
                    })
                    .await
                    .map_err(|error| {
                        AppError::internal(format!(
                            "Le processus de traitement a été interrompu : {error}"
                        ))
                    })
                    .and_then(|result| result)
                },
                move |result| Message::JobFinished(id, result),
            ));
        }
        Task::batch(tasks)
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        let waiting = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Waiting))
            .count();
        let running = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Running))
            .count();
        let done = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Done(_)))
            .count();
        let failed = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Failed(_)))
            .count();
        (waiting, running, done, failed)
    }
}
