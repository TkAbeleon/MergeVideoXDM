//! Présentation Iced : aucune logique de parsing XDM ou d'exécution FFmpeg.
use iced::widget::svg::{self, Svg};
use iced::widget::{
    button, checkbox, column, container, pick_list, progress_bar, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length};

use super::{App, Job, JobState, Message};

const ICON_APP: &[u8] = include_bytes!("../../assets/icons/app-mark.svg");
const ICON_FOLDER: &[u8] = include_bytes!("../../assets/icons/folder.svg");
const ICON_FOLDER_PLUS: &[u8] = include_bytes!("../../assets/icons/folder-plus.svg");
const ICON_OUTPUT: &[u8] = include_bytes!("../../assets/icons/output.svg");
const ICON_PLAY: &[u8] = include_bytes!("../../assets/icons/play.svg");
const ICON_TRASH: &[u8] = include_bytes!("../../assets/icons/trash.svg");
const ICON_CHECK: &[u8] = include_bytes!("../../assets/icons/check.svg");
const ICON_ALERT: &[u8] = include_bytes!("../../assets/icons/alert.svg");
const ICON_CLOCK: &[u8] = include_bytes!("../../assets/icons/clock.svg");

pub(super) fn view(app: &App) -> Element<'_, Message> {
    let ffmpeg_banner: Element<'_, Message> = match &app.ffmpeg_status {
        Ok(version) => container(
            row![
                icon(ICON_CHECK, 18.0),
                column![
                    text("FFmpeg est disponible").size(13),
                    text(version).size(11)
                ]
                .spacing(2)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(12)
        .style(container::success)
        .width(Length::Fill)
        .into(),
        Err(error) => container(
            row![
                icon(ICON_ALERT, 18.0),
                column![
                    text(error.kind.label()).size(13),
                    text(error.to_string()).size(11),
                    text(error.hint.clone().unwrap_or_default()).size(11)
                ]
                .spacing(3),
                space().width(Length::Fill)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(12)
        .style(container::warning)
        .width(Length::Fill)
        .into(),
    };

    let header = row![
        icon(ICON_APP, 54.0),
        column![
            text("MergeVideoXDM").size(29),
            text("Assembler les segments XDM avec le FFmpeg du système").size(13)
        ]
        .spacing(3),
        space().width(Length::Fill),
        container(text("Adaptateurs XDM").size(12))
            .style(container::secondary)
            .padding([7, 11])
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    let add_button = button(
        row![
            icon(ICON_FOLDER_PLUS, 18.0),
            text("Ajouter plusieurs dossiers")
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .style(button::primary)
    .padding([10, 14])
    .on_press(Message::AddFolders);

    let start_button = button(
        row![icon(ICON_PLAY, 17.0), text("Démarrer / relancer")]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .style(button::primary)
    .padding([10, 14])
    .on_press(Message::StartQueue);

    let toolbar = row![
        add_button,
        space().width(Length::Fill),
        text("Traitements simultanés").size(12),
        pick_list([1_usize, 2, 3, 4], Some(app.parallelism), Message::ParallelismChanged)
            .placeholder("Nombre")
            .width(Length::Fixed(72.0)),
        start_button
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let output_card = container(
        row![
            icon(ICON_OUTPUT, 22.0),
            column![
                text("Dossier de sortie").size(13),
                text(app.output_dir.to_string_lossy().to_string()).size(12)
            ]
            .spacing(3),
            space().width(Length::Fill),
            button(text("Parcourir…"))
                .style(button::secondary)
                .on_press(Message::PickOutputFolder)
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding(14)
    .style(container::rounded_box)
    .width(Length::Fill);

    let counts = app.counts();
    let summary = row![
        text(format!("{} dossier(s)", app.jobs.len())).size(13),
        text("·").size(13),
        text(format!("{} en cours", counts.1)).size(13),
        text("·").size(13),
        text(format!("{} terminé(s)", counts.2)).size(13),
        text("·").size(13),
        text(format!("{} erreur(s)", counts.3)).size(13),
        space().width(Length::Fill),
        checkbox(app.include_incomplete)
            .label("Autoriser une récupération partielle")
            .on_toggle(Message::IncludeIncompleteChanged)
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let queue_body: Element<'_, Message> = if app.jobs.is_empty() {
        container(
            column![
                icon(ICON_FOLDER_PLUS, 36.0),
                text("La file d'attente est vide").size(18),
                text("Sélectionne plusieurs dossiers de tâches XDM en une seule opération. Chaque tâche doit contenir chunks.db.1 ou chunks.db.2 et les segments temporaires associés.").size(13),
                button("Sélectionner des dossiers")
                    .style(button::secondary)
                    .on_press(Message::AddFolders)
            ]
            .spacing(12)
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .padding(44)
        .style(container::rounded_box)
        .into()
    } else {
        let cards = column(app.jobs.iter().map(job_card).collect::<Vec<_>>())
            .spacing(10)
            .width(Length::Fill);
        scrollable(cards).height(Length::Fill).into()
    };

    let notice_view: Element<'_, Message> = if let Some(notice) = &app.notice {
        container(
            row![
                text(notice).size(12),
                space().width(Length::Fill),
                button("Masquer")
                    .style(button::text)
                    .on_press(Message::DismissNotice)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([9, 12])
        .style(container::secondary)
        .width(Length::Fill)
        .into()
    } else {
        space().height(Length::Fixed(0.0)).into()
    };

    container(
        column![
            header,
            ffmpeg_banner,
            toolbar,
            output_card,
            summary,
            queue_body,
            notice_view,
            text("Progression lue depuis FFmpeg · Détails et erreurs par tâche · Les sources XDM ne sont jamais modifiées").size(11)
        ]
        .spacing(16)
        .padding(22)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn job_card<'a>(job: &'a Job) -> Element<'a, Message> {
    let snapshot = job.snapshot();
    let running = matches!(job.state, JobState::Running);
    let name_input = text_input("nom-de-la-video.mp4", &job.target_name)
        .on_input(move |value| Message::TargetNameChanged(job.id, value))
        .width(Length::Fixed(218.0));

    let remove_button = button(
        row![icon(ICON_TRASH, 15.0), text("Retirer")]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .style(button::subtle)
    .on_press_maybe((!running).then_some(Message::RemoveJob(job.id)))
    .padding([7, 10]);

    let folder_label = job
        .source_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Dossier XDM")
        .to_string();

    let title = row![
        icon(ICON_FOLDER, 20.0),
        column![
            text(folder_label).size(15),
            text(job.source_dir.to_string_lossy().to_string()).size(11)
        ]
        .spacing(3)
        .width(Length::Fill),
        column![text("Nom du fichier cible").size(11), name_input].spacing(4),
        remove_button
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let state: Element<'_, Message> = match &job.state {
        JobState::Waiting => row![
            icon(ICON_CLOCK, 16.0),
            text("En attente").size(12)
        ]
        .spacing(7)
        .align_y(Alignment::Center)
        .into(),
        JobState::Running => row![
            icon(ICON_PLAY, 16.0),
            text(snapshot.stage.clone()).size(12),
            space().width(Length::Fill),
            text(format!("{:.0} %", snapshot.fraction * 100.0)).size(12)
        ]
        .spacing(7)
        .align_y(Alignment::Center)
        .into(),
        JobState::Done(report) => {
            let label = if report.partial {
                "Terminé — récupération partielle"
            } else {
                "Assemblage terminé"
            };
            column![
                row![
                    icon(ICON_CHECK, 16.0),
                    text(label).size(12),
                    space().width(Length::Fill),
                    text("100 %").size(12)
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                text(format!(
                    "{} · {} segment(s) · XDM {}",
                    report.output.display(),
                    report.segment_count,
                    report.xdm_version
                ))
                .size(11),
                text(report.detail.clone()).size(11)
            ]
            .spacing(5)
            .into()
        }
        JobState::Failed(error) => row![
            icon(ICON_ALERT, 16.0),
            column![
                text(error.kind.label()).size(12),
                text(error.message.clone()).size(11)
            ]
            .spacing(4),
            space().width(Length::Fill),
            button("Réessayer")
                .style(button::secondary)
                .on_press(Message::RetryJob(job.id))
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    };

    let progress = progress_bar(0.0..=1.0, snapshot.fraction)
        .height(Length::Fixed(7.0));

    let details_label = if job.details_open {
        "Masquer les détails"
    } else {
        "Voir les détails"
    };

    let details: Element<'_, Message> = if job.details_open {
        let mut rows = column![text("Journal de cette tâche").size(12)].spacing(5);
        for line in &snapshot.logs {
            rows = rows.push(text(line.clone()).size(11));
        }
        if let JobState::Failed(error) = &job.state {
            if let Some(hint) = &error.hint {
                rows = rows.push(text(format!("Conseil : {hint}")).size(11));
            }
        }
        container(scrollable(rows).height(Length::Fixed(150.0)))
            .padding(10)
            .style(container::secondary)
            .width(Length::Fill)
            .into()
    } else {
        space().height(Length::Fixed(0.0)).into()
    };

    container(
        column![
            title,
            state,
            progress,
            row![
                text(format!(
                    "Étape : {}",
                    if snapshot.stage.is_empty() {
                        "En attente"
                    } else {
                        &snapshot.stage
                    }
                ))
                .size(11),
                space().width(Length::Fill),
                button(text(details_label))
                    .style(button::text)
                    .on_press(Message::ToggleDetails(job.id))
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            details
        ]
        .spacing(10),
    )
    .padding(14)
    .style(container::rounded_box)
    .width(Length::Fill)
    .into()
}

fn icon<'a>(source: &'static [u8], size: f32) -> Svg<'a> {
    Svg::new(svg::Handle::from_memory(source))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
}
