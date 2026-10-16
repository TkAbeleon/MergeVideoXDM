mod merge;

use iced::widget::svg::{self, Svg};
use iced::widget::{button, checkbox, column, container, pick_list, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length, Task, Theme};
use merge::{MergeReport, MergeRequest};
use std::path::PathBuf;

const ICON_APP: &[u8] = include_bytes!("../assets/icons/app-mark.svg");
const ICON_FOLDER: &[u8] = include_bytes!("../assets/icons/folder.svg");
const ICON_FOLDER_PLUS: &[u8] = include_bytes!("../assets/icons/folder-plus.svg");
const ICON_OUTPUT: &[u8] = include_bytes!("../assets/icons/output.svg");
const ICON_PLAY: &[u8] = include_bytes!("../assets/icons/play.svg");
const ICON_TRASH: &[u8] = include_bytes!("../assets/icons/trash.svg");
const ICON_CHECK: &[u8] = include_bytes!("../assets/icons/check.svg");
const ICON_ALERT: &[u8] = include_bytes!("../assets/icons/alert.svg");
const ICON_CLOCK: &[u8] = include_bytes!("../assets/icons/clock.svg");

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("MergeVideoXDM")
        .theme(|_| None::<Theme>)
        .window_size(iced::Size::new(1080.0, 820.0))
        .run()
}

#[derive(Debug, Clone)]
enum JobState {
    Waiting,
    Running,
    Done { report: MergeReport },
    Failed(String),
}

#[derive(Debug, Clone)]
struct Job {
    id: u64,
    source_dir: PathBuf,
    target_name: String,
    state: JobState,
}

#[derive(Debug, Clone)]
enum Message {
    AddFolder,
    PickOutputFolder,
    TargetNameChanged(u64, String),
    RemoveJob(u64),
    StartQueue,
    RetryJob(u64),
    JobFinished(u64, Result<MergeReport, String>),
    ParallelismChanged(usize),
    IncludeIncompleteChanged(bool),
    DismissNotice,
}

struct App {
    jobs: Vec<Job>,
    output_dir: PathBuf,
    parallelism: usize,
    include_incomplete: bool,
    queue_started: bool,
    next_id: u64,
    ffmpeg_status: Result<String, String>,
    notice: Option<String>,
}

impl App {
    fn new() -> Self {
        let videos = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Videos")
            .join("MergeVideoXDM");
        Self {
            jobs: Vec::new(),
            output_dir: videos,
            parallelism: 2,
            include_incomplete: false,
            queue_started: false,
            next_id: 1,
            ffmpeg_status: merge::ffmpeg_info(),
            notice: None,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddFolder => {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Choisir un dossier temporaire XDM contenant chunks.db.1/.2")
                    .pick_folder()
                {
                    self.add_folder(folder);
                }
                Task::none()
            }
            Message::PickOutputFolder => {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Choisir le dossier des vidéos assemblées")
                    .pick_folder()
                {
                    self.output_dir = folder;
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
                if self.jobs.iter().any(|job| job.id == id && matches!(job.state, JobState::Running)) {
                    self.notice = Some("Un traitement en cours ne peut pas être retiré.".into());
                } else {
                    self.jobs.retain(|job| job.id != id);
                }
                Task::none()
            }
            Message::StartQueue => {
                self.queue_started = true;
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
                    }
                }
                self.launch_available()
            }
            Message::JobFinished(id, result) => {
                match result {
                    Ok(report) => {
                        self.notice = Some(format!("Assemblage terminé : {}", report.output.display()));
                        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                            job.state = JobState::Done { report };
                        }
                    }
                    Err(error) => {
                        self.notice = Some(format!("Un traitement a échoué : {}", first_line(&error)));
                        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                            job.state = JobState::Failed(error);
                        }
                    }
                }
                if self.queue_started { self.launch_available() } else { Task::none() }
            }
            Message::ParallelismChanged(value) => {
                self.parallelism = value.clamp(1, 4);
                if self.queue_started { self.launch_available() } else { Task::none() }
            }
            Message::IncludeIncompleteChanged(value) => {
                self.include_incomplete = value;
                Task::none()
            }
            Message::DismissNotice => {
                self.notice = None;
                Task::none()
            }
        }
    }

    fn add_folder(&mut self, folder: PathBuf) {
        let normalized = folder.canonicalize().unwrap_or_else(|_| folder.clone());
        if self.jobs.iter().any(|job| job.source_dir.canonicalize().unwrap_or_else(|_| job.source_dir.clone()) == normalized) {
            self.notice = Some("Ce dossier est déjà dans la file d'attente.".into());
            return;
        }
        let folder_name = folder.file_name().and_then(|name| name.to_str()).unwrap_or("video");
        let safe_name: String = folder_name.chars().map(|character| {
            if character == '/' || character == '\\' || character == '\0' { '_' } else { character }
        }).collect();
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(Job {
            id,
            source_dir: folder,
            target_name: format!("{}.mp4", if safe_name.is_empty() { format!("video-{id}") } else { safe_name }),
            state: JobState::Waiting,
        });
        self.notice = None;
    }

    fn launch_available(&mut self) -> Task<Message> {
        let active = self.jobs.iter().filter(|job| matches!(job.state, JobState::Running)).count();
        let slots = self.parallelism.saturating_sub(active);
        if slots == 0 { return Task::none(); }

        let waiting: Vec<u64> = self.jobs.iter()
            .filter(|job| matches!(job.state, JobState::Waiting))
            .take(slots)
            .map(|job| job.id)
            .collect();
        if waiting.is_empty() { return Task::none(); }

        let mut tasks = Vec::with_capacity(waiting.len());
        for id in waiting {
            let request = if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                job.state = JobState::Running;
                Some(MergeRequest {
                    source_dir: job.source_dir.clone(),
                    output_dir: self.output_dir.clone(),
                    target_name: job.target_name.clone(),
                    include_incomplete: self.include_incomplete,
                })
            } else { None };
            let Some(request) = request else { continue; };
            tasks.push(Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || merge::merge(request)).await
                        .map_err(|error| format!("Le traitement a été interrompu : {error}"))
                        .and_then(|result| result)
                },
                move |result| Message::JobFinished(id, result),
            ));
        }
        Task::batch(tasks)
    }

    fn view(&self) -> Element<'_, Message> {
        let ffmpeg_banner: Element<'_, Message> = match &self.ffmpeg_status {
            Ok(version) => container(row![
                icon(ICON_CHECK, 18.0),
                column![text("FFmpeg est disponible").size(13), text(version).size(11)]
            ].spacing(10).align_y(Alignment::Center))
                .padding(12).style(container::success).width(Length::Fill).into(),
            Err(reason) => container(row![
                icon(ICON_ALERT, 18.0),
                column![text("FFmpeg n'a pas été détecté").size(13), text(reason).size(11)]
                    .spacing(2),
                space().width(Length::Fill),
                text("Installe FFmpeg et redémarre l'application.").size(11)
            ].spacing(10).align_y(Alignment::Center))
                .padding(12).style(container::warning).width(Length::Fill).into(),
        };

        let header = row![
            icon(ICON_APP, 54.0),
            column![
                text("MergeVideoXDM").size(29),
                text("Récupérer l'ordre des segments XDM et les assembler avec FFmpeg").size(13)
            ].spacing(3),
            space().width(Length::Fill),
            container(text("XDM 8.0.29").size(12)).style(container::secondary).padding([7, 11])
        ].spacing(14).align_y(Alignment::Center);

        let add_button = button(row![icon(ICON_FOLDER_PLUS, 18.0), text("Ajouter un dossier XDM")].spacing(8).align_y(Alignment::Center))
            .style(button::primary).padding([10, 14]).on_press(Message::AddFolder);
        let start_button = button(row![icon(ICON_PLAY, 17.0), text("Démarrer / relancer")].spacing(8).align_y(Alignment::Center))
            .style(button::primary).padding([10, 14]).on_press(Message::StartQueue);

        let parallel_picker = pick_list([1_usize, 2, 3, 4], Some(self.parallelism), Message::ParallelismChanged)
            .placeholder("Nombre")
            .width(Length::Fixed(72.0));
        let toolbar = row![
            add_button,
            space().width(Length::Fill),
            text("Traitements en parallèle").size(12),
            parallel_picker,
            start_button
        ].spacing(10).align_y(Alignment::Center);

        let output_card = container(row![
            icon(ICON_OUTPUT, 22.0),
            column![
                text("Dossier de sortie").size(13),
                text(self.output_dir.to_string_lossy().to_string()).size(12)
            ].spacing(3),
            space().width(Length::Fill),
            button(text("Parcourir…")).style(button::secondary).on_press(Message::PickOutputFolder)
        ].spacing(12).align_y(Alignment::Center))
            .padding(14).style(container::rounded_box).width(Length::Fill);

        let counts = self.counts();
        let summary = row![
            text(format!("{} dossier(s)", self.jobs.len())).size(13),
            text("·").size(13), text(format!("{} en cours", counts.1)).size(13),
            text("·").size(13), text(format!("{} terminé(s)", counts.2)).size(13),
            text("·").size(13), text(format!("{} erreur(s)", counts.3)).size(13),
            space().width(Length::Fill),
            checkbox("Autoriser une récupération partielle", self.include_incomplete)
                .on_toggle(Message::IncludeIncompleteChanged)
        ].spacing(8).align_y(Alignment::Center);

        let queue_body: Element<'_, Message> = if self.jobs.is_empty() {
            container(column![
                icon(ICON_FOLDER_PLUS, 36.0),
                text("La file d'attente est vide").size(18),
                text("Ajoute le dossier temporaire d'un téléchargement XDM. Il doit contenir chunks.db.1 ou chunks.db.2 et les segments temporaires.").size(13),
                button("Choisir un dossier").style(button::secondary).on_press(Message::AddFolder)
            ].spacing(12).align_x(Alignment::Center))
                .center_x(Length::Fill).padding(44).style(container::rounded_box).into()
        } else {
            let cards = column(self.jobs.iter().map(|job| self.job_card(job)).collect::<Vec<_>>())
                .spacing(10).width(Length::Fill);
            scrollable(cards).height(Length::Fill).into()
        };

        let notice_view: Element<'_, Message> = if let Some(notice) = &self.notice {
            container(row![
                text(notice).size(12),
                space().width(Length::Fill),
                button("Masquer").style(button::text).on_press(Message::DismissNotice)
            ].spacing(10).align_y(Alignment::Center))
                .padding([9, 12]).style(container::secondary).width(Length::Fill).into()
        } else {
            space().height(Length::Fixed(0.0)).into()
        };

        container(column![
            header,
            ffmpeg_banner,
            toolbar,
            output_card,
            summary,
            queue_body,
            notice_view,
            text("Mode sûr par défaut · Copie des flux sans réencodage · Les sources XDM ne sont jamais modifiées").size(11)
        ].spacing(16).padding(22).width(Length::Fill).height(Length::Fill))
            .width(Length::Fill).height(Length::Fill)
            .into()
    }

    fn job_card(&self, job: &Job) -> Element<'_, Message> {
        let running = matches!(job.state, JobState::Running);
        let name_input = text_input("nom-de-la-video.mp4", &job.target_name)
            .on_input(move |value| Message::TargetNameChanged(job.id, value))
            .width(Length::Fixed(218.0));
        let remove_button = button(row![icon(ICON_TRASH, 15.0), text("Retirer")].spacing(6).align_y(Alignment::Center))
            .style(button::subtle)
            .on_press_maybe((!running).then_some(Message::RemoveJob(job.id)))
            .padding([7, 10]);
        let folder_label = job.source_dir.file_name().and_then(|name| name.to_str())
            .unwrap_or("Dossier XDM").to_string();
        let title = row![
            icon(ICON_FOLDER, 20.0),
            column![
                text(folder_label).size(15),
                text(job.source_dir.to_string_lossy().to_string()).size(11)
            ].spacing(3),
            space().width(Length::Fill),
            column![text("Nom du fichier cible").size(11), name_input].spacing(4),
            remove_button
        ].spacing(12).align_y(Alignment::Center);

        let state: Element<'_, Message> = match &job.state {
            JobState::Waiting => row![icon(ICON_CLOCK, 16.0), text("En attente").size(12)].spacing(7).align_y(Alignment::Center).into(),
            JobState::Running => row![icon(ICON_PLAY, 16.0), text("FFmpeg assemble les segments…").size(12)]
                .spacing(7).align_y(Alignment::Center).into(),
            JobState::Done { report } => {
                let label = if report.partial { "Terminé avec une sortie partielle" } else { "Assemblage terminé" };
                column![
                    row![icon(ICON_CHECK, 16.0), text(label).size(12)].spacing(7).align_y(Alignment::Center),
                    text(format!("{} · {} segment(s) · {}", report.output.display(), report.segment_count, report.detail)).size(11)
                ].spacing(4).into()
            }
            JobState::Failed(error) => row![
                icon(ICON_ALERT, 16.0),
                column![text("Échec du traitement").size(12), text(error.clone()).size(11)].spacing(4),
                space().width(Length::Fill),
                button("Réessayer").style(button::secondary).on_press(Message::RetryJob(job.id))
            ].spacing(8).align_y(Alignment::Center).into(),
        };

        container(column![title, state].spacing(10))
            .padding(14).style(container::rounded_box).width(Length::Fill).into()
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        let waiting = self.jobs.iter().filter(|job| matches!(job.state, JobState::Waiting)).count();
        let running = self.jobs.iter().filter(|job| matches!(job.state, JobState::Running)).count();
        let done = self.jobs.iter().filter(|job| matches!(job.state, JobState::Done { .. })).count();
        let failed = self.jobs.iter().filter(|job| matches!(job.state, JobState::Failed(_))).count();
        (waiting, running, done, failed)
    }
}

fn icon<'a>(source: &'static [u8], size: f32) -> Svg<'a> {
    svg::svg(svg::Handle::from_memory(source))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}
