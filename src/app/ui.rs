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
    let _refresh_sequence = app.tick_nonce;
    let counts = app.counts();

    let ffmpeg_banner: Element<'_, Message> = match &app.ffmpeg_status {
        Ok(version) => container(
            row![
                icon(ICON_CHECK, 19.0),
                column![
                    text("FFmpeg est prêt").size(13),
                    text(version).size(11)
                ]
                .spacing(2),
                space().width(Length::Fill),
                text("Moteur système").size(11)
            ]
            .spacing(11)
            .align_y(Alignment::Center),
        )
        .padding([11, 14])
        .style(container::success)
        .width(Length::Fill)
        .into(),
        Err(error) => container(
            row![
                icon(ICON_ALERT, 19.0),
                column![
                    text(error.kind.label()).size(13),
                    text(error.to_string()).size(11),
                    text(error.hint.clone().unwrap_or_default()).size(11)
                ]
                .spacing(3),
                space().width(Length::Fill)
            ]
            .spacing(11)
            .align_y(Alignment::Center),
        )
        .padding([11, 14])
        .style(container::warning)
        .width(Length::Fill)
        .into(),
    };

    let header = row![
        icon(ICON_APP, 52.0),
        column![
            text("MergeVideoXDM").size(29),
            text("Récupérer les vidéos XDM sans réencodage").size(13)
        ]
        .spacing(3),
        space().width(Length::Fill),
        container(
            column![
                text("LINUX").size(10),
                text("XDM 8.0.29").size(12)
            ]
            .spacing(2)
            .align_x(Alignment::End)
        )
        .padding([7, 12])
        .style(container::secondary)
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    let add_button = button(
        row![
            icon(ICON_FOLDER_PLUS, 18.0),
            text("Ajouter des dossiers")
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .style(button::secondary)
    .padding([10, 14])
    .on_press(Message::AddFolders);

    let start_label = if counts.0 > 0 {
        "Lancer les fusions"
    } else if counts.3 > 0 {
        "Relancer les échecs"
    } else {
        "Traiter la file"
    };

    let start_button = button(
        row![icon(ICON_PLAY, 17.0), text(start_label)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .style(button::primary)
    .padding([10, 14])
    .on_press_maybe((!app.jobs.is_empty()).then_some(Message::StartQueue));

    let toolbar = row![
        add_button,
        space().width(Length::Fill),
        text("Simultanés").size(12),
        pick_list([1_usize, 2, 3, 4], Some(app.parallelism), Message::ParallelismChanged)
            .placeholder("Nombre")
            .width(Length::Fixed(82.0)),
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
            .spacing(3)
            .width(Length::Fill),
            button(text("Modifier…"))
                .style(button::secondary)
                .on_press(Message::PickOutputFolder)
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([12, 14])
    .style(container::rounded_box)
    .width(Length::Fill);

    let metrics = row![
        metric_card("En attente", counts.0, ICON_CLOCK),
        metric_card("En cours", counts.1, ICON_PLAY),
        metric_card("Terminées", counts.2, ICON_CHECK),
        metric_card("Échecs", counts.3, ICON_ALERT)
    ]
    .spacing(10)
    .width(Length::Fill);

    let queue_heading = row![
        column![
            text("File de fusion").size(18),
            text(format!("{} tâche(s) au total", app.jobs.len())).size(12)
        ]
        .spacing(3),
        space().width(Length::Fill),
        checkbox(app.include_incomplete)
            .label("Autoriser la récupération partielle")
            .on_toggle(Message::IncludeIncompleteChanged)
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let queue_body: Element<'_, Message> = if app.jobs.is_empty() {
        container(
            column![
                icon(ICON_FOLDER_PLUS, 42.0),
                text("Aucune tâche pour le moment").size(19),
                text("Ajoute les dossiers temporaires des téléchargements XDM que tu veux récupérer. Chaque tâche garde son propre nom de sortie, sa progression et son diagnostic.")
                    .size(13),
                button(
                    row![
                        icon(ICON_FOLDER_PLUS, 17.0),
                        text("Sélectionner des dossiers")
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .style(button::primary)
                .padding([10, 14])
                .on_press(Message::AddFolders),
                text("Astuce : commence par 1 ou 2 traitements simultanés sur un disque lent.")
                    .size(11)
            ]
            .spacing(13)
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .padding(38)
        .style(container::rounded_box)
        .width(Length::Fill)
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
            metrics,
            queue_heading,
            queue_body,
            notice_view,
            text("Traitement local · Les fichiers source XDM ne sont ni modifiés ni supprimés.")
                .size(11)
        ]
        .spacing(13)
        .padding(22)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn metric_card<'a>(
    label: &'static str,
    value: usize,
    icon_source: &'static [u8],
) -> Element<'a, Message> {
    container(
        row![
            icon(icon_source, 21.0),
            column![
                text(label).size(11),
                text(value.to_string()).size(23)
            ]
            .spacing(3)
        ]
        .spacing(11)
        .align_y(Alignment::Center),
    )
    .padding([10, 13])
    .style(container::secondary)
    .width(Length::Fill)
    .into()
}

fn job_card<'a>(job: &'a Job) -> Element<'a, Message> {
    let snapshot = job.snapshot();
    let running = matches!(job.state, JobState::Running);
    let name_input = text_input("ma-video.mp4", &job.target_name)
        .on_input(move |value| Message::TargetNameChanged(job.id, value))
        .width(Length::Fixed(270.0));

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
        icon(ICON_FOLDER, 21.0),
        column![
            text(folder_label).size(15),
            text(job.source_dir.to_string_lossy().to_string()).size(11)
        ]
        .spacing(3)
        .width(Length::Fill),
        remove_button
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let target_row = row![
        column![text("Nom du fichier de sortie").size(11), name_input].spacing(4),
        space().width(Length::Fill),
        text("Aucun écrasement de fichier existant").size(10)
    ]
    .spacing(12)
    .align_y(Alignment::End);

    let state: Element<'_, Message> = match &job.state {
        JobState::Waiting => status_chip("En attente".into(), ICON_CLOCK, ChipTone::Neutral),
        JobState::Running => status_chip(
            snapshot.stage.clone(),
            ICON_PLAY,
            ChipTone::Active,
        ),
        JobState::Done(report) => {
            let label = if report.partial {
                "Terminé — récupération partielle"
            } else {
                "Assemblage terminé"
            };
            let tone = if report.partial {
                ChipTone::Warning
            } else {
                ChipTone::Success
            };
            let badge = status_chip(label.into(), if report.partial { ICON_ALERT } else { ICON_CHECK }, tone);
            column![
                badge,
                text(format!(
                    "Sortie : {} · {} segment(s) · XDM {}",
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
            status_chip(error.kind.label().to_string(), ICON_ALERT, ChipTone::Warning),
            column![
                text(error.message.clone()).size(11),
                text(error.hint.clone().unwrap_or_default()).size(11)
            ]
            .spacing(3)
            .width(Length::Fill),
            button("Réessayer")
                .style(button::secondary)
                .on_press(Message::RetryJob(job.id))
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    };

    let progress = progress_bar(0.0..=1.0, snapshot.fraction);

    let stage_name = if snapshot.stage.is_empty() {
        "En attente".to_string()
    } else {
        snapshot.stage.clone()
    };

    let details_label = if job.details_open {
        "Masquer les détails"
    } else {
        "Journal et diagnostic"
    };

    let details: Element<'_, Message> = if job.details_open {
        let mut rows = column![text("Journal de cette tâche").size(12)].spacing(5);
        if snapshot.logs.is_empty() {
            rows = rows.push(text("Aucun événement enregistré pour le moment.").size(11));
        }
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
            target_row,
            state,
            progress,
            row![
                text(format!("Étape actuelle : {stage_name}")).size(11),
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
    .padding(15)
    .style(container::rounded_box)
    .width(Length::Fill)
    .into()
}

#[derive(Clone, Copy)]
enum ChipTone {
    Neutral,
    Active,
    Success,
    Warning,
}

fn status_chip<'a>(
    label: String,
    icon_source: &'static [u8],
    tone: ChipTone,
) -> Element<'a, Message> {
    let content = row![icon(icon_source, 16.0), text(label).size(12)]
        .spacing(7)
        .align_y(Alignment::Center);

    match tone {
        ChipTone::Neutral => container(content)
            .padding([6, 9])
            .style(container::secondary)
            .into(),
        ChipTone::Active => container(content)
            .padding([6, 9])
            .style(container::primary)
            .into(),
        ChipTone::Success => container(content)
            .padding([6, 9])
            .style(container::success)
            .into(),
        ChipTone::Warning => container(content)
            .padding([6, 9])
            .style(container::warning)
            .into(),
    }
}

fn icon<'a>(source: &'static [u8], size: f32) -> Svg<'a> {
    Svg::new(svg::Handle::from_memory(source))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
}
