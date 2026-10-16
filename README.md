# MergeVideoXDM

**Application de bureau Linux légère pour assembler les segments HLS temporaires de XDM avec le FFmpeg installé sur le système.**

MergeVideoXDM utilise Rust et Iced, une interface graphique native sans navigateur intégré. Les icônes SVG sont embarquées dans le binaire. X11 et Wayland sont activés, avec rendu logiciel Tiny-Skia pour éviter l'obligation d'un GPU récent.

> **État du projet :** version initiale en cours de validation. Le support fonctionnel est actuellement limité au format de métadonnées de **XDM 8.0.29**. L'architecture permet d'ajouter d'autres formats, mais chaque format doit être implémenté et testé. La compatibilité avec une autre version n'est pas affirmée avant vérification sur des fichiers réels de cette version.

## Fonctionnalités

- Sélection de **plusieurs dossiers en une seule opération**.
- File d'attente avec état indépendant pour chaque dossier.
- Nom de fichier cible et chemin de sortie configurables.
- Traitement configurable de 1 à 4 tâches en parallèle.
- Barre de progression par tâche basée sur les événements de progression de FFmpeg ; l'étape de traitement est également affichée.
- Vue détaillée individuelle : journal, étape en cours, diagnostic, cause et conseil pour résoudre les erreurs.
- Catégories d'erreurs explicites : état XDM illisible ou non reconnu, segments manquants, FFmpeg absent ou échoué, nom de sortie invalide, conflit de sortie et erreur de fichier.
- Mode sûr par défaut ; récupération partielle facultative, identifiée clairement.
- Assemblage sans réencodage : FFmpeg copie les flux et ne convertit pas la vidéo ou l'audio.
- Aucune modification ni suppression des fichiers source XDM.
- Refus d'écraser une sortie existante, y compris si un autre processus la crée pendant le traitement.

## Compatibilité

| Composant | État |
|---|---|
| Linux x86_64 | Cible CI |
| Linux ARM64 / aarch64 | Cible CI |
| X11 | Activé |
| Wayland | Activé |
| FFmpeg | Fourni par le système |
| XDM 8.0.29 | Adaptateur implémenté ; tests unitaires du format |
| Autres versions XDM | Architecture prête pour des adaptateurs ; compatibilité non validée |

La compatibilité du binaire avec différentes distributions dépend aussi de leurs bibliothèques système et portails de sélection de dossiers. La CI Linux est configurée pour compiler sur les runners x86_64 et ARM64 ; elle ne garantit pas que chaque paquet de chaque distribution sera binaire-compatible.

## Installation de FFmpeg

Le programme ne fournit pas de copie privée de FFmpeg. La commande **ffmpeg** doit être accessible via **PATH**.

Fedora (si le paquet est disponible dans les dépôts configurés) :

    sudo dnf install ffmpeg

Debian / Ubuntu :

    sudo apt update
    sudo apt install ffmpeg

Arch Linux :

    sudo pacman -S ffmpeg

## Utilisation

1. Mets en pause le téléchargement concerné dans XDM et ferme XDM proprement.
2. Sauvegarde son dossier temporaire avant toute récupération.
3. Lance MergeVideoXDM.
4. Clique sur **Ajouter plusieurs dossiers** et sélectionne tous les dossiers de tâches voulus dans le sélecteur.
5. Choisis le dossier de sortie commun, puis ajuste le nom cible pour chaque tâche.
6. Choisis le nombre de traitements simultanés (commence par 1 ou 2 sur un PC modeste).
7. Clique sur **Démarrer / relancer**. Déplie **Voir les détails** sur une tâche pour consulter son journal ou le diagnostic d'une erreur.

Un dossier de tâche XDM est habituellement situé sous :

    ~/.xdm-app-data/Data/temp/<identifiant-de-tache>/

Le dossier doit contenir **chunks.db.1** ou **chunks.db.2** et les segments temporaires associés. Ajoute le dossier spécifique de la tâche, pas son dossier parent temp.

Les extensions de sortie actuellement autorisées sont .mp4, .mkv, .ts, .mov et .m4v. Le codec n'est pas réencodé automatiquement : si un conteneur ne convient pas aux codecs de la source, FFmpeg peut refuser le traitement. Un conteneur MKV ou TS peut être plus adapté selon le flux.

### Comprendre la progression

La barre est calculée à partir des événements de progression émis par FFmpeg, rapportés à la durée cumulée indiquée par la base XDM. Certaines variantes de flux ne donnent pas une durée fiable ; dans ce cas, la progression peut rester basse ou avancer par à-coups. La barre atteint 100 % uniquement quand FFmpeg termine avec succès et que la sortie a été validée. Une tâche en échec ne doit jamais être interprétée comme terminée.

### Récupération partielle

Par défaut, tous les segments doivent être présents et marqués Finished. Si des fichiers manquent ou sont incomplets, le programme n'assemble rien en mode sûr et affiche les problèmes détectés.

L'option **Autoriser une récupération partielle** tente d'assembler les segments présents. Le résultat peut être tronqué, présenter des coupures ou s'arrêter avant la fin. Cette option ne peut pas reconstituer des données jamais téléchargées.

## Architecture modulaire

    src/
    ├── main.rs           Entrée du programme
    ├── app/
    │   ├── mod.rs        État UI, file d'attente, multitâche et progression
    │   └── ui.rs         Présentation Iced et vues individuelles
    ├── domain.rs         Requêtes, rapports et valeurs de progression partagées
    ├── errors.rs         Erreurs typées et diagnostics lisibles
    ├── ffmpeg.rs         Processus FFmpeg, manifeste, progression et stderr
    ├── merge.rs          Orchestration indépendante de XDM
    └── xdm/
        ├── mod.rs        Contrat d'adaptateur et détection des formats
        └── v829.rs       Lecteur de l'état binaire XDM 8.0.29

Les icônes SVG sont dans assets/icons/. Le packaging Linux et la CI sont dans packaging/ et .github/workflows/.

### Ajouter une autre version de XDM

Le moteur de fusion ne doit pas analyser directement les structures binaires d'une version particulière. Chaque version doit se trouver dans un adaptateur qui implémente le contrat XdmStateAdapter :

1. Créer un nouveau module dans src/xdm/ (par exemple v830.rs).
2. Implémenter version(), recognizes() et parse().
3. Convertir le format de cette version vers le modèle commun XdmState et XdmSegment, en conservant strictement l'ordre des segments fourni par XDM.
4. Enregistrer l'adaptateur dans src/xdm/mod.rs.
5. Ajouter des fixtures représentatives et des tests d'ordre, de corruption, de versions non reconnues et de chemins de segments.
6. Vérifier la récupération sur un dossier de tâche réel de cette version avant de déclarer la compatibilité.

Un adaptateur ne doit pas déduire l'ordre en triant les noms de fichiers. Il doit utiliser la source d'ordre réellement enregistrée par la version concernée.

## Compilation depuis les sources

### Prérequis

- Rust stable 1.88 ou plus récent (édition 2024).
- FFmpeg pour exécuter et tester le traitement.
- Compilateur C, pkg-config, dépendances de développement X11/Wayland/DBus.

Fedora :

    sudo dnf install gcc gcc-c++ make pkgconf-pkg-config libxkbcommon-devel libxkbcommon-x11-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel wayland-devel libxcb-devel dbus-devel

Debian / Ubuntu :

    sudo apt install build-essential pkg-config libx11-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcb-randr0-dev libxcb-xinerama0-dev libxcb-cursor-dev libdbus-1-dev

Arch Linux :

    sudo pacman -S --needed base-devel pkgconf libx11 libxkbcommon libxkbcommon-x11 wayland libxcb dbus

Construire :

    git clone https://github.com/TkAbeleon/MergeVideoXDM.git
    cd MergeVideoXDM
    cargo fmt --all -- --check
    cargo test --all-targets
    cargo build --release
    ./target/release/mergevideoxdm

Pour une machine modeste, limite le nombre de fusions simultanées à 1 ou 2 : chaque tâche peut lire plusieurs segments sur le disque et démarrer des processus FFmpeg.

## CI et publications

Le workflow Linux CI vérifie le formatage Rust, lance les tests et compile des binaires pour Linux x86_64 et ARM64. Le workflow Release prépare des archives Linux sur un tag v*, avec un fichier SHA-256.

Créer une publication :

    git tag v0.1.0
    git push origin v0.1.0

Les artefacts et le résultat réel des pipelines sont consultables dans [GitHub Actions](https://github.com/TkAbeleon/MergeVideoXDM/actions).

## Limites fonctionnelles

- L'adaptateur implémenté cible le format d'état de XDM 8.0.29.
- Le programme ne déchiffre pas les flux HLS AES-128.
- Les playlists fMP4 nécessitant le segment d'initialisation #EXT-X-MAP ne sont pas encore reconstruites.
- La progression peut être approximative si la durée cumulée des segments est absente ou incorrecte.
- Les codecs incompatibles avec le conteneur demandé peuvent faire échouer le mode copie ; aucun réencodage n'est lancé automatiquement.
- Les dialogues de sélection multiple dépendent du backend fourni par rfd et de la configuration du bureau Linux.

## Données et sécurité

MergeVideoXDM ne contacte pas de service externe. Il lit l'état XDM et les segments, puis écrit la sortie dans le dossier choisi. Il ne modifie pas et ne supprime pas les fichiers temporaires sources. Garde cependant une sauvegarde de la tâche XDM avant récupération.

## Licence

Licence MIT — voir LICENSE.
