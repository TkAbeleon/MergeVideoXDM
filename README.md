# MergeVideoXDM

**Interface graphique légère pour récupérer et assembler les segments HLS temporaires de XDM 8.0.29 avec le FFmpeg du système.**

MergeVideoXDM lit l'ordre des segments conservé par XDM dans `chunks.db.1` ou `chunks.db.2`, puis demande à FFmpeg de les assembler dans cet ordre, sans réencodage. L'application est écrite en Rust avec Iced ; son interface est native (pas de navigateur intégré) et contient ses propres icônes SVG. Elle prend en charge X11 et Wayland.

> **Important :** ce programme vise le format de téléchargement de XDM **8.0.29**. Il ne doit pas être considéré comme compatible avec les versions réécrites qui utilisent un autre format d'état.

## Fonctions

- Ajout de plusieurs dossiers temporaires XDM dans une file d'attente.
- Traitement de 1 à 4 dossiers en parallèle, selon les ressources disponibles.
- Nom de sortie configurable par tâche et sélection d'un dossier de sortie commun.
- Détection de FFmpeg au démarrage et affichage des erreurs de traitement.
- Mode sûr par défaut : une tâche avec segments absents ou non terminés n'est pas assemblée comme si elle était complète.
- Mode de récupération partielle, explicitement activable, pour tenter de sauver les segments réellement présents.
- Assemblage par FFmpeg avec `-c copy` : aucune conversion vidéo/audio n'est demandée.
- Pour les tâches contenant des pistes vidéo et audio distinctes, création de pistes temporaires puis multiplexage sans réencodage.
- Les fichiers d'origine `chunks.db.*` et les segments de XDM ne sont jamais modifiés ni supprimés.
- Dialogues de dossiers via XDG Desktop Portal lorsque le bureau Linux le prend en charge.

## Captures et versions

Les builds Linux sont publiés dans l'onglet [Releases](https://github.com/TkAbeleon/MergeVideoXDM/releases). Les archives de compilation comprennent le binaire, le fichier `.desktop`, l'icône et un script d'installation utilisateur.

## Utilisation

### 1. Installer FFmpeg

FFmpeg doit être disponible dans `PATH` et exécutable par la commande `ffmpeg`.

Fedora (dépôt activé proposant le paquet) :

```bash
sudo dnf install ffmpeg
```

Debian / Ubuntu :

```bash
sudo apt update
sudo apt install ffmpeg
```

Arch Linux :

```bash
sudo pacman -S ffmpeg
```

Les noms de paquets et les dépôts peuvent varier selon la distribution. Le programme ne fournit pas de copie privée de FFmpeg.

### 2. Mettre le téléchargement XDM à l'abri

1. Dans XDM, mets le téléchargement concerné en pause.
2. Ferme XDM proprement avant d'inspecter ou de récupérer ses données.
3. Fais une copie de sauvegarde du dossier temporaire concerné.

Le dossier à ajouter est normalement de la forme :

```text
~/.xdm-app-data/Data/temp/<identifiant-de-tache>/
```

Ce dossier doit contenir `chunks.db.1` ou `chunks.db.2` ainsi que les segments temporaires associés. **Sélectionne le dossier de tâche qui contient ces fichiers**, pas le dossier parent `temp` ni le dossier `recovered` créé par un autre outil.

### 3. Assembler les vidéos

1. Lance `mergevideoxdm`.
2. Clique sur **Ajouter un dossier XDM** et sélectionne le dossier temporaire d'une tâche. Répète l'opération pour ajouter d'autres tâches.
3. Choisis le dossier de sortie. Par défaut, il s'agit de `~/Videos/MergeVideoXDM`.
4. Pour chaque tâche, saisis un nom de fichier cible, par exemple `ma-video.mp4`. L'extension autorisée doit être `.mp4`, `.mkv`, `.ts`, `.mov` ou `.m4v`.
5. Choisis le nombre de traitements simultanés. Pour une machine modeste, commence par 1 ou 2.
6. Clique sur **Démarrer / relancer**.

Le programme refuse d'écraser un fichier de sortie existant. Choisis un autre nom ou déplace le résultat déjà présent. Les tâches qui réussissent affichent le chemin du fichier créé ; celles qui échouent présentent un message et peuvent être relancées séparément.

### Mode sûr et récupération partielle

Par défaut, tous les segments doivent être présents et marqués `Finished` dans l'état XDM. Si cette vérification échoue, aucune sortie n'est créée pour cette tâche.

L'option **Autoriser une récupération partielle** permet de tenter l'assemblage avec les segments présents même lorsque certains segments sont incomplets ou absents. La sortie sera explicitement indiquée comme partielle ; elle peut présenter des coupures, des artefacts ou s'arrêter avant la fin. Cette option ne recrée pas les données non téléchargées.

## Limites connues

- La base `chunks.db.*` est ici interprétée comme le format binaire de XDM 8.0.29. Le programme n'essaie pas de déduire arbitrairement l'ordre depuis les noms de fichiers.
- Les segments HLS classiques, notamment `.ts`, sont la cible principale.
- Le programme ne déchiffre pas les playlists HLS AES-128 et ne reconstruit pas les playlists fMP4 qui nécessitent un segment d'initialisation `#EXT-X-MAP`.
- Aucun réencodage n'est lancé. Les codecs incompatibles avec le conteneur demandé peuvent donc faire échouer FFmpeg ; essaie `.mkv` ou `.ts` selon le type de flux.
- L'interface ne prétend pas afficher un pourcentage de progression de FFmpeg lorsqu'il n'est pas mesuré. Une tâche reste indiquée comme en cours jusqu'au retour du processus.
- Pour les fenêtres de sélection de dossier via portail XDG, un portail compatible du bureau (par exemple GTK ou KDE) doit être installé. Si le portail n'est pas disponible, la distribution peut nécessiter `zenity`.

## Compilation depuis les sources

### Prérequis

- Rust stable 1.88 ou plus récent (l'édition 2024 est utilisée).
- FFmpeg pour les tests manuels et l'exécution.
- Les bibliothèques de développement nécessaires à X11 / Wayland et à `pkg-config`.

Fedora :

```bash
sudo dnf install gcc gcc-c++ make pkgconf-pkg-config libxkbcommon-devel libxkbcommon-x11-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel wayland-devel libxcb-devel dbus-devel
```

Debian / Ubuntu :

```bash
sudo apt install build-essential pkg-config libx11-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcb-randr0-dev libxcb-xinerama0-dev libxcb-cursor-dev libdbus-1-dev
```

Arch Linux :

```bash
sudo pacman -S --needed base-devel pkgconf libx11 libxkbcommon libxkbcommon-x11 wayland libxcb dbus
```

### Construire et lancer

```bash
git clone https://github.com/TkAbeleon/MergeVideoXDM.git
cd MergeVideoXDM
cargo test
cargo build --release
./target/release/mergevideoxdm
```

Iced utilise le moteur de rendu logiciel `tiny-skia` pour ne pas imposer un GPU moderne. Les backends de fenêtre X11 et Wayland sont activés. La construction depuis les sources requiert néanmoins les dépendances de développement indiquées ci-dessus.

## CI et artefacts Linux

GitHub Actions exécute le formatage, les tests et la compilation sur Linux x86_64 et ARM64. Le workflow de publication est déclenché par un tag de version `v*` et publie une archive Linux pour chaque architecture construite.

Exemple de création d'une version :

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Arborescence du projet

```text
src/main.rs          Interface Iced, file d'attente et multitâche
src/merge.rs         Lecture de chunks.db et orchestration FFmpeg
assets/icons/        Icônes SVG intégrées et icône de l'application
packaging/           Entrée de menu et script d'installation utilisateur
.github/workflows/   CI Linux et publication des artefacts
```

## Sécurité des données

MergeVideoXDM ne contacte aucun service externe, ne modifie pas les métadonnées de XDM et ne supprime aucun segment source. Tous les résultats sont écrits dans le dossier de sortie choisi. Avant récupération d'un téléchargement en cours ou bloqué, effectue tout de même une sauvegarde du dossier temporaire XDM.

## Licence

Le projet est sous licence MIT. Voir [LICENSE](LICENSE).
