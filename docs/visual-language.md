# Flowmates — langage visuel, système esthétique et prompts pour IA

> Référence de design réutilisable pour reproduire l’esthétique de Flowmates dans d’autres applications.
>
> État audité : 27 juillet 2026  
> Application source : Flowmates pour macOS 14+  
> Fenêtre de référence : `370 × 700 px`, redimensionnable, minimum `340 × 400 px`  
> Sources observées : renderer HTML/CSS/JavaScript et rendu local au viewport réel de l’application.

---

## 1. Rôle de ce document

Ce document ne décrit pas seulement des couleurs ou des composants. Il formalise une **grammaire visuelle complète** : la sensation générale de l’interface, ses proportions, ses matériaux, sa hiérarchie, ses états et le vocabulaire à employer pour qu’un designer ou une autre IA puisse la recréer sans voir le projet original.

Il peut servir de :

- brief de direction artistique ;
- spécification pour une IA de génération d’interface ;
- base de design system pour un autre produit ;
- guide de revue visuelle ;
- référence pour décrire un screenshot Flowmates avec précision ;
- catalogue de prompts par composant ;
- garde-fou pour conserver l’identité visuelle pendant une migration technique.

Le but n’est pas de recopier chaque pixel partout. Le but est de préserver les invariants qui rendent Flowmates reconnaissable : **calme, clarté, rondeur, progression, confidentialité et concentration**.

---

## 2. L’esthétique Flowmates en une phrase

> Une interface de productivité macOS claire et apaisée, construite comme un tableau de bord personnel très compact, avec un grand indicateur temporel circulaire, des cartes blanches flottantes, une palette presque monochrome réveillée par un violet électrique et des visualisations de données simples, tactiles et optimistes.

### 2.1 Mots-clés principaux

- macOS natif sans imitation excessive du chrome système ;
- productivité personnelle ;
- calme et confiance ;
- fond blanc cassé ;
- cartes blanches aérées ;
- violet comme signal principal ;
- formes circulaires et rayons généreux ;
- données lisibles, jamais intimidantes ;
- très faible bruit visuel ;
- interface compacte à hiérarchie presque mobile ;
- feedback discret ;
- confidentialité locale ;
- progression plutôt que performance agressive.

### 2.2 Mots-clés à éviter

- dashboard financier dense ;
- cyberpunk ;
- néon ;
- verre très transparent ;
- gradients partout ;
- ombres lourdes ;
- noir pur en grandes surfaces ;
- skeuomorphisme réaliste ;
- gamification enfantine ;
- interface corporate froide ;
- panneaux bordés en cascade ;
- tableaux surchargés ;
- couleurs de statut utilisées comme décoration.

---

## 3. Description fidèle du screenshot actuel

### 3.1 Contexte exact

Le screenshot de référence a été observé dans un viewport de `370 × 700 px`, identique à la taille initiale définie pour la fenêtre macOS. Le chrome natif de la fenêtre n’est pas inclus dans la surface web inspectée.

État visible :

- onglet **Today** actif ;
- heure de suivi à `0:00` ;
- objectif quotidien réglé sur `6 h` ;
- suivi arrêté ;
- série à `0 days Streak` ;
- tâche courante non renseignée ;
- navigation inférieure visible ;
- contenu de la carte de tâche partiellement sous la ligne de flottaison.

Le contour orangé autour de l’onglet Today aperçu après interaction est un **artefact de focus du navigateur de développement**, pas une couleur de marque. Le focus produit doit être cohérent avec le violet principal ou un focus système accessible explicitement assumé.

### 3.2 Description courte, type texte alternatif

> Écran étroit d’une application de suivi du temps sur fond blanc cassé. Un immense anneau gris très pâle occupe le centre supérieur ; il contient l’objectif « Daily goal 6h », le temps « 0:00 » en grands chiffres noirs et un bouton Play blanc circulaire. Sous l’anneau figurent la série de jours, une carte de sélection d’objectif, une carte de tâche, puis une barre de navigation inférieure à quatre icônes, avec Today sélectionné en violet.

### 3.3 Description longue destinée à une IA visuelle

> Créer l’écran principal d’une application macOS de productivité dans une fenêtre verticale compacte de 370 × 700 px. Utiliser un fond global blanc cassé très clair `#FAFAFA`, sans texture. Placer en haut à gauche une salutation discrète en gris ardoise, taille 13 px. Faire dominer la composition par un anneau de progression parfaitement circulaire, presque aussi large que la fenêtre disponible, avec une piste épaisse gris bleuté très pâle. L’anneau est centré, généreusement entouré d’espace vide. Dans son centre, empiler un petit libellé gris « Daily goal 6h », un temps « 0:00 » très large, gras, noir bleuté, puis un bouton Play blanc de 56 px, circulaire, légèrement surélevé par une ombre douce. Sous le cercle, afficher « 0 days Streak » en gras, centré. Ajouter ensuite des cartes blanches de largeur complète avec coins de 16 px et ombre presque imperceptible : une carte pour le sélecteur d’objectif quotidien et une carte pour la tâche courante. Fixer en bas une navigation blanche à quatre entrées avec icônes au trait et petits libellés : Today, Summary, Pro, Profile. L’état actif est violet sur fond lavande très pâle. L’ensemble doit paraître calme, précis, privé, tactile et natif à macOS, avec énormément d’air et aucun élément décoratif superflu.

### 3.4 Structure spatiale du screenshot

```text
┌──────────────────────────────────────┐
│  Good night, there!                  │  ← texte secondaire, 13 px
│                                      │
│       ╭──────────────────────╮       │
│    ╭──╯                      ╰──╮    │
│   │       Daily goal 6h          │   │
│   │           0:00               │   │  ← anneau temporel dominant
│   │            (▶)               │   │
│    ╰──╮                      ╭──╯    │
│       ╰──────────────────────╯       │
│                                      │
│            0 days Streak             │
│                                      │
│  ╭────────────────────────────────╮  │
│  │ Daily goal                     │  │
│  │ [ 6 hours                   ▾ ] │  │
│  ╰────────────────────────────────╯  │
│                                      │
│  ╭────────────────────────────────╮  │
│  │ Current Task                   │  │
│  │ [ What are you working on?   ] │  │
│  ╰────────────────────────────────╯  │
├──────────────────────────────────────┤
│    ◷          ǁ          ✣       ♙   │
│  Today     Summary       Pro   Profile│  ← navigation fixe
└──────────────────────────────────────┘
```

### 3.5 Hiérarchie perçue

1. Le temps actuel `0:00`.
2. L’anneau qui représente l’objectif et la progression.
3. Le bouton Play, action principale immédiate.
4. La série de jours.
5. L’objectif quotidien et la tâche.
6. La navigation persistante.
7. La salutation, volontairement secondaire.

Cette hiérarchie est importante : la marque ne cherche pas à impressionner par un titre ou un logo. Elle met l’**action personnelle présente** au centre.

---

## 4. Principes de composition

### 4.1 Une seule chose dominante par écran

Chaque vue possède un centre de gravité clair :

- Today : l’anneau temporel ;
- Summary : le temps total et la semaine ;
- Pro : la conversation ;
- Profile : l’identité et l’état local/cloud ;
- Report : la santé globale et la synthèse exécutive ;
- Onboarding : la question courante.

Ne jamais placer deux composants de même poids visuel au-dessus de la ligne de flottaison.

### 4.2 Le vide est un composant

Le fond clair autour de l’anneau ne doit pas être rempli avec des statistiques supplémentaires. Cet espace communique :

- calme ;
- absence de surveillance agressive ;
- concentration ;
- simplicité ;
- contrôle personnel.

### 4.3 La rondeur exprime la continuité

Les cercles, anneaux, pastilles, pills et cartes arrondies sont utilisés pour évoquer :

- le temps cyclique ;
- la progression ;
- des objectifs non punitifs ;
- une interaction douce ;
- une application personnelle plutôt qu’un outil de contrôle managérial.

### 4.4 La couleur est fonctionnelle

Le violet sert à répondre à trois questions seulement :

1. Où suis-je ?
2. Qu’est-ce qui progresse ?
3. Quelle est l’action principale ?

Les autres couleurs sont réservées aux catégories de données et aux statuts.

### 4.5 Les données restent humaines

Les métriques importantes utilisent des durées lisibles (`2h 10m`, `52m`), des pourcentages courts et des phrases d’interprétation. Les graphiques accompagnent une explication ; ils ne remplacent pas l’explication.

---

## 5. Design tokens

### 5.1 Palette fondamentale

| Token | HSL source | Hex approximatif | Usage |
|---|---:|---:|---|
| Background | `0 0% 98%` | `#FAFAFA` | fond général |
| Foreground | `222.2 84% 4.9%` | `#020817` | texte principal, chiffres clés |
| Card | `0 0% 100%` | `#FFFFFF` | cartes, surfaces élevées |
| Primary | `262 83% 58%` | `#7C3BED` | progression, sélection, CTA |
| Primary foreground | `210 40% 98%` | `#F8FAFC` | texte sur violet |
| Secondary / Muted | `210 40% 96.1%` | `#F1F5F9` | pistes, fonds doux, états neutres |
| Secondary foreground | `222.2 47.4% 11.2%` | `#0F172A` | texte secondaire fort |
| Muted foreground | `215.4 16.3% 46.9%` | `#64748B` | labels, sous-titres, aide |
| Accent | `262 83% 96%` | `#F3ECFD` | fond de sélection lavande |
| Accent foreground | `262 83% 40%` | `#4F11BB` | texte violet sombre |
| Destructive | `0 84.2% 60.2%` | `#EF4444` | arrêt, erreur, risque |
| Border / Input | `214.3 31.8% 91.4%` | `#E2E8F0` | séparateurs et champs |

### 5.2 Palette de visualisation

| Couleur | Hex approximatif | Fonction recommandée |
|---|---:|---|
| Violet | `#7C3BED` | catégorie principale, progression |
| Magenta | `#DD3CA7` | deuxième catégorie |
| Bleu | `#2463EB` | troisième catégorie, information |
| Turquoise | `#2EB8AA` | focus profond, stabilité |
| Vert | proche `#2EAD63` | objectif atteint, succès |
| Jaune | proche `#F5C51B` | sixième catégorie, attention douce |
| Orange | `#F97415` | objectif en retard, focus à améliorer |
| Rouge | `#EF4444` | erreur ou risque réel uniquement |

Règle : ne pas employer toute la palette dans un écran sans données. Un écran simple doit rester presque monochrome.

### 5.3 Typographie

Pile de fontes :

```css
font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
```

Échelle observée et recommandée :

| Rôle | Taille | Graisse | Détail |
|---|---:|---:|---|
| Temps principal | `48 px` | `700` | chiffres tabulaires, tracking `-1 px` |
| Total résumé | `32 px` | `700` | interlettrage `-1 px` |
| Titre de vue | `28 px` | `700` | interlettrage `-0.5 px` |
| Titre onboarding | `22 px` | `700` | tracking `-0.03em` |
| Titre login | `21 px` | `700` | tracking `-0.03em` |
| Titre modal consentement | `20 px` | `700` | ligne `1.25` |
| Titre de rapport | `18–20 px` | `700` | sobre, compact |
| Titre de section | `14–18 px` | `600–700` | selon importance |
| Texte courant | `12–14 px` | `400–500` | ligne `1.45–1.55` |
| Label | `10–12 px` | `500–600` | couleur secondaire |
| Métadonnée | `9–11 px` | `500–700` | parfois uppercase |

Principes :

- les chiffres clés sont foncés, gras et compacts ;
- les labels sont petits mais jamais décoratifs ;
- les titres utilisent un tracking légèrement négatif ;
- les données temporelles utilisent des chiffres tabulaires ;
- l’uppercase est réservé aux kickers, statuts et en-têtes de tableau ;
- éviter les paragraphes centrés hors onboarding, login et empty states.

### 5.4 Espacement

L’interface emploie une trame compacte dérivée de `2, 4, 6, 8, 10, 12, 14, 16, 20, 24 px`.

Règles pratiques :

- marge horizontale de vue : `16 px` ;
- espacement standard entre cartes : `12 px` ;
- padding de carte compact : `14–16 px` ;
- padding de carte importante : `20 px` ;
- gap interne de contrôle : `6–8 px` ;
- séparation de grandes sections : `16–20 px` ;
- réserve sous le contenu : `80 px` pour éviter la navigation fixe.

### 5.5 Rayons

| Élément | Rayon |
|---|---:|
| Bouton/puce capsule | `999 px` |
| Bouton Play | `50%` |
| Carte principale | `20 px` |
| Carte secondaire | `16 px` |
| Carte rapport | `12–16 px` |
| Bulle de chat | `14 px`, coin de queue à `4 px` |
| Input / bouton compact | environ `14–18 px` |
| Petit contrôle carré | `8–12 px` |

Le rayon global source est `1.25rem`, soit environ `20 px`, mais les composants compacts descendent volontairement à `12–16 px`.

### 5.6 Ombres et élévation

Niveaux recommandés :

```css
/* Carte standard */
box-shadow: 0 2px 8px rgb(0 0 0 / 0.05);

/* Carte principale */
box-shadow: 0 2px 12px rgb(0 0 0 / 0.06);

/* Bouton circulaire */
box-shadow: 0 2px 12px rgb(0 0 0 / 0.10);

/* Bouton circulaire survolé */
box-shadow: 0 6px 24px rgb(0 0 0 / 0.16);

/* Modal */
box-shadow: 0 24px 48px rgb(0 0 0 / 0.20);
```

Une carte Flowmates ne doit jamais avoir une ombre noire dure. L’ombre indique une légère séparation de matière, pas une profondeur théâtrale.

### 5.7 Bordures

- couleur standard : `#E2E8F0` ;
- épaisseur : `1 px` ;
- certaines cartes utilisent uniquement une ombre ;
- un élément sélectionné reçoit une bordure violette et éventuellement un halo violet à 20 % ;
- les séparateurs sont fins et peu contrastés ;
- ne pas doubler systématiquement bordure et ombre.

### 5.8 Icônes

- style : pictogrammes SVG au trait, simples et géométriques ;
- épaisseur : généralement `2 px` ;
- extrémités et jointures arrondies ;
- taille navigation : `20–26 px` ;
- taille inline : `14–16 px` ;
- taille icône de progression : `22 px` ;
- les emojis ne sont acceptés que dans l’onboarding et certaines catégories explicatives ;
- éviter les icônes pleines lourdes, sauf Play/Pause/Stop.

### 5.9 Mouvement

Le mouvement est rapide et discret :

- interaction simple : `150–180 ms` ;
- progression d’anneau : `400–500 ms ease` ;
- progression onboarding : `250 ms ease` ;
- entrée toast : `300 ms ease-out` ;
- spinner : `600–800 ms linear infinite` ;
- hover d’icône : scale maximum `1.12` ;
- pression : scale `0.96`.

Il ne doit pas y avoir de rebond, de parallaxe ou d’animation décorative continue.

---

## 6. Cadre d’application et comportement responsive

### 6.1 Fenêtre macOS

- taille initiale : `370 × 700 px` ;
- largeur minimale : `340 px` ;
- hauteur minimale : `400 px` ;
- redimensionnable ;
- thème clair ;
- contenu principal centré ;
- scrolling vertical limité à la zone de contenu ;
- navigation inférieure fixe.

### 6.2 Shell

Le shell principal accepte une largeur maximale de `900 px`, mais l’identité est conçue d’abord pour une fenêtre étroite. Sur une grande fenêtre, le contenu doit rester centré et ne pas s’étirer comme un dashboard plein écran.

### 6.3 Zone de contenu

- `16 px` de padding latéral ;
- `16 px` en haut ;
- `80 px` en bas pour la navigation ;
- largeur utile à 370 px : environ `338 px` ;
- aucun débordement horizontal ;
- les cartes occupent toute la largeur utile.

### 6.4 Adaptation sous 420 px

Les rapports passent progressivement de grilles multiples à une colonne :

- métriques : 4 vers 2 colonnes ;
- blocs doubles : 2 vers 1 colonne ;
- breakdown : colonnes vers empilement ;
- tableau de rapport : masque les colonnes secondaires plutôt que de réduire le texte à l’illisible.

### 6.5 Principe de transfert

Dans un autre projet desktop, conserver la largeur de lecture principale entre `340 et 520 px` pour les parcours personnels. Pour une application web large, centrer un rail principal plutôt que d’agrandir l’anneau et les cartes à l’infini.

---

## 7. Catalogue détaillé des composants

## 7.1 Anneau temporel principal — « la roue avec le temps »

### Rôle

L’anneau temporel est la signature visuelle centrale. Ce n’est pas un cadran analogique ni une molette manipulable : c’est un **indicateur radial de progression vers un objectif quotidien**, qui contient l’action de suivi.

### Anatomie

1. Conteneur carré responsive, ratio `1:1`.
2. Piste circulaire neutre.
3. Arc de progression violet.
4. Petit label d’objectif au centre.
5. Temps cumulé en très grands chiffres.
6. Bouton Play/Pause blanc, flottant.

### Géométrie source

- largeur : `100%` de la zone, maximum `500 px` ;
- SVG : `viewBox 0 0 200 200` ;
- centre : `(100, 100)` ;
- rayon : `82` ;
- circonférence : environ `515.22` ;
- épaisseur de piste : `14` ;
- arc démarrant à midi grâce à une rotation de `-90°` ;
- extrémités d’arc arrondies ;
- bouton central : `56 × 56 px`.

### États

| État | Aspect |
|---|---|
| Sans objectif | piste complète pâle, label « No goal target » |
| Objectif défini, arrêté | piste pâle, progression calculée, bouton Play |
| Actif | arc violet progresse, icône Pause |
| Chargement | icône atténuée et spinner fin superposé |
| Désactivé | bouton à 60 % d’opacité |
| Objectif atteint | arc complet ; possibilité d’ajouter un feedback vert discret sans recolorer tout le cercle |

### Raison esthétique

Le cercle transforme le suivi du temps en objet calme et contemplatif. Sa grande taille donne de l’importance au temps sans afficher plusieurs métriques concurrentes.

### Prompt IA du composant

> Grand anneau radial de suivi du temps, minimaliste et centré, piste épaisse gris bleuté très pâle, arc de progression violet saturé à extrémités arrondies partant de midi, contenu central composé d’un petit objectif gris, d’une durée noire en chiffres tabulaires très gras et d’un bouton Play blanc circulaire de 56 px avec ombre douce. Beaucoup d’espace vide, aucun tick, aucune aiguille, aucun gradient dans l’anneau.

### À ne pas faire

- ajouter des graduations de montre ;
- utiliser plusieurs arcs concurrents ;
- mettre un gradient arc-en-ciel ;
- placer des statistiques autour du cercle ;
- transformer le cercle en bouton entier ;
- utiliser une ombre interne sombre ;
- afficher des millisecondes.

---

## 7.2 Semaine-calendrier

### Rôle

Le calendrier de Flowmates est une bande hebdomadaire compacte, pas une grille mensuelle. Il donne une lecture immédiate de la continuité d’activité.

### Anatomie

- 7 colonnes équidistantes ;
- lettre ou abréviation du jour au-dessus ;
- cercle de `32 × 32 px` sous le label ;
- `6 px` entre label et cercle ;
- ligne entière avec marges latérales de `4 px` ;
- espace inférieur de `20 px` avant la carte suivante.

### États

| État du jour | Cercle | Label |
|---|---|---|
| Inactif | blanc, bordure gris clair `2 px` | gris ardoise |
| Activité enregistrée | fond violet, bordure violette, coche blanche | gris ardoise |
| Aujourd’hui sans activité | blanc, bordure violette `2.5 px` | foncé et gras |
| Aujourd’hui avec activité | fond violet, coche blanche, label foncé et gras | foncé et gras |

### Lecture visuelle

Le composant ressemble davantage à un **habit tracker adulte** qu’à un date picker. Il communique la régularité, pas la planification d’événements.

### Prompt IA du composant

> Bande de calendrier hebdomadaire minimaliste sur une seule ligne, sept jours répartis régulièrement. Chaque jour possède une petite abréviation grise et un cercle de 32 px. Les jours terminés sont des pastilles violettes contenant une coche blanche ; le jour courant est marqué par un contour violet plus épais et un label plus sombre. Aucun quadrillage, aucun en-tête de mois, aucun événement textuel.

### Variante pour un autre projet

Conserver le rythme `label + cercle`, mais remplacer la coche par :

- un point pour une habitude ;
- un chiffre pour une date ;
- une petite icône de statut ;
- un anneau partiel pour une progression quotidienne.

Ne pas combiner les quatre dans la même version.

---

## 7.3 Navigation inférieure

### Anatomie

- barre blanche fixée en bas ;
- fine bordure supérieure ;
- quatre entrées centrées ;
- gap responsive `24 px` à `120 px` ;
- item minimum `56 px` ;
- icône au trait au-dessus, label dessous ;
- `3 px` entre icône et texte ;
- padding vertical compact ;
- réserve de contenu de `80 px`.

### États

- normal : gris ardoise ;
- hover : violet + fond muted ;
- actif : violet + fond lavande ;
- pression : scale `0.96` ;
- Pro verrouillé : label légèrement atténué, mais toujours lisible.

### Caractère

La navigation emprunte une hiérarchie mobile tout en vivant dans une fenêtre desktop étroite. C’est un choix volontaire : les quatre destinations sont permanentes, simples et accessibles au pouce ou à la souris.

### Prompt IA du composant

> Barre de navigation inférieure fixe, blanche, très fine, bordure supérieure gris clair, quatre destinations espacées et centrées. Chaque destination combine une icône linéaire arrondie de 22 px et un libellé de 10 px. L’item actif est violet sur une petite surface lavande aux coins de 12 px. Pas de barre sombre, pas d’onglet en forme de capsule géante, pas de badge rouge décoratif.

---

## 7.4 Cartes de réglage Today

### Structure

- surface blanche ;
- rayon `16 px` ;
- padding `14 px` ;
- ombre `0 2px 8px / 5%` ;
- marge basse `12 px` ;
- label `12 px`, graisse `600` ;
- input ou select de largeur complète.

La carte Goal est courte. La carte Current Task peut inclure un select, un input manuel et une checkbox.

### Prompt IA

> Carte de réglage compacte blanche sur fond blanc cassé, coins de 16 px, ombre très légère, padding de 14 px. Petit label sombre semi-gras, puis champ pleine largeur à fond presque blanc, bordure ardoise très claire et coins généreux. Densité modérée, aucun header coloré.

---

## 7.5 Boutons

### Primaire

- fond violet ;
- texte blanc cassé ;
- rayon généreux ;
- hauteur compacte ;
- aucun gradient ;
- hover par légère variation d’opacité.

### Secondaire

- fond `#F1F5F9` ;
- texte bleu-noir ;
- bordure gris clair ;
- hover lavande pâle.

### Ghost

- fond transparent ;
- texte sombre ;
- hover lavande pâle.

### Destructif

- rouge réservé à l’action Stop, aux erreurs ou à un risque ;
- l’action Stop du timer reste blanche avec texte rouge afin d’éviter une présence agressive constante.

### Bouton Play

Le Play est une exception : bouton blanc circulaire, icône gris ardoise, ombre douce. L’action principale est signifiée par sa position et sa forme, pas par un aplat violet.

---

## 7.6 Champs, selects et checkboxes

### Champs

- fond global très clair ;
- bordure `1 px` ;
- texte `12 px` ;
- padding `6 × 10 px` en densité normale ;
- focus violet + halo violet à 20 % ;
- placeholder gris ardoise.

### Selects

Même matériau que les champs. Le chevron natif peut rester visible s’il respecte la densité macOS.

### Checkbox

- `14 × 14 px` dans Today ;
- `16 × 16 px` dans les réglages de consentement ;
- accent violet ;
- label de `11–13 px`.

### Règle IA

> Les formulaires Flowmates doivent paraître légers et intégrés aux cartes, jamais comme des rectangles industriels. Leur contraste de bordure est faible au repos et net au focus.

---

## 7.7 Écran Summary

### Composition

1. Titre `Summary`, 28 px.
2. Bouton pill `Work report` à droite.
3. Date centrée, 13 px gris.
4. Semaine-calendrier.
5. Carte principale du temps.
6. Titre `Highlights`.
7. Carte Deep Focus.
8. Carte Distractions.
9. Timeline.

### Carte principale du temps

- rayon `20 px` ;
- padding `20 px` ;
- petit anneau `72 × 72 px` ;
- piste `6 px` ;
- icône sablier au centre ;
- total à `32 px / 700` ;
- point vert ou orange pour l’état de l’objectif ;
- barres de tâches en dessous.

### Prompt IA de l’écran

> Écran de synthèse quotidien clair et vertical. En haut, titre noir « Summary », bouton pill lavande « Work report », date grise centrée et bande hebdomadaire de sept pastilles. Puis une grande carte blanche arrondie contenant un petit anneau violet de progression, le temps total en gros chiffres et des barres horizontales colorées pour la répartition des tâches. Sous la carte, deux cartes de highlights avec une phrase interprétative, un petit histogramme turquoise du focus et un compteur de distractions. Style éditorial léger, beaucoup d’air, visualisations simples.

---

## 7.8 Anneau de progression secondaire

Il reprend la forme de la grande roue sans chercher à la concurrencer :

- `72 × 72 px` ;
- rayon SVG `30` ;
- trait `6 px` ;
- arc violet ;
- piste muted ;
- icône sablier violette au centre ;
- placé à gauche d’une valeur temporelle.

Dans un autre projet, ce composant sert pour une progression de quota, de session ou d’objectif. Il doit toujours être accompagné d’une valeur textuelle explicite.

---

## 7.9 Barres de répartition des tâches

### Anatomie

- pourcentage à gauche dans une colonne de `28 px` ;
- piste de hauteur `28 px`, rayon `8 px` ;
- durée à droite dans une colonne de `36 px` ;
- label dans la barre si elle est assez longue ;
- label à l’extérieur si la barre est trop courte ;
- maximum de six catégories visibles ;
- largeur relative calculée sur la catégorie la plus longue ;
- pourcentage calculé sur le temps total.

### Prompt IA

> Liste de barres horizontales de répartition, compactes et arrondies, avec pourcentage gris à gauche, durée sombre à droite et libellé placé intelligemment à l’intérieur en blanc ou juste après la barre en foncé. Palette alternant violet, magenta, bleu, turquoise, vert et jaune. Pas de légende séparée, pas d’axes.

---

## 7.10 Graphique Deep Focus

- 12 colonnes représentant 8 h à 20 h ;
- hauteur totale `52 px` ;
- barres turquoise ;
- gap `3 px` ;
- coins supérieurs arrondis ;
- barres sans activité en muted à 55 % ;
- axes réduits aux labels `8am` et `8pm` ;
- caption explicative en dessous.

Le graphique doit évoquer une **texture temporelle**, pas une analyse scientifique. Les tooltips donnent les détails au survol.

---

## 7.11 Timeline d’activité

### Apparence

- rail vertical gris clair de `2 px` ;
- point de `8 px` à chaque entrée ;
- heure et durée sur la première ligne ;
- catégorie en semi-gras ;
- ticket optionnel sous forme de badge ;
- description à `11 px`, ligne `1.6` ;
- espacement régulier, séparateurs implicites.

### Prompt IA

> Timeline verticale sobre avec un rail gris très fin et de petits points ronds. Chaque événement présente l’heure à gauche, la durée à droite, une catégorie semi-grasse puis une description compacte en gris sombre. Les tickets sont de petites pastilles vertes. Aucun avatar, aucune grande icône, aucune couleur par événement.

---

## 7.12 Écran de connexion / activation cloud

### Direction artistique

L’écran de connexion est plus promotionnel que le reste de l’app, mais garde la même matière.

### Hero de cartes superposées

Trois mini-cartes donnent un aperçu du produit :

1. carte Summary blanche, inclinée de `-3°` ;
2. carte Streak violette, inclinée de `+4°` ;
3. carte Work report blanche, presque horizontale, au premier plan.

La carte Streak utilise exceptionnellement un gradient violet et une ombre violette plus expressive. Les jours sont des mini-pastilles de `16 px`.

### Formulaire

- largeur maximale `400 px` ;
- titre centré `21 px` ;
- tagline `12 px` ;
- carte d’authentification blanche, rayon `16 px` ;
- champs email et mot de passe ;
- bouton primaire ;
- séparateur `or continue with` avec deux lignes ;
- bouton Google secondaire ;
- lien local/free en ghost.

### Prompt IA

> Splash de connexion compact pour application macOS. Partie haute avec trois mini-cartes de dashboard superposées et légèrement inclinées : une carte blanche de barres de focus, une carte violette de streak et une notification blanche de rapport prêt. Partie basse centrée avec titre, tagline et formulaire blanc arrondi. Fond gris très pâle, aspect premium mais amical, pas de grande illustration 3D.

---

## 7.13 Onboarding en cinq étapes

### Structure

- overlay plein écran sur fond clair ;
- bouton Retour carré de `32 px` ;
- barre de progression de `6 px`, pill ;
- titre `22 px` ;
- sous-titre `13 px` gris ;
- liste verticale d’options ;
- footer fixe avec gradient vers transparent ;
- bouton Continue pleine largeur.

### Carte d’option

- disposition horizontale ;
- rayon `16 px` ;
- bordure claire ;
- padding `14 px` ;
- cercle de sélection `22 px` à gauche ;
- texte au centre ;
- emoji de `22 px` à droite ;
- sélection : bordure violette, halo discret, cercle violet avec coche.

### Ton

Les questions sont courtes, personnelles et positives. Le composant ressemble à un questionnaire de préférences, pas à un assistant bavard.

### Prompt IA

> Onboarding macOS minimaliste en cinq étapes, fond blanc cassé, petite barre de progression violette en haut, question large et concise, sous-titre gris, puis options sous forme de cartes blanches arrondies empilées. Chaque option possède un cercle de sélection à gauche et un emoji discret à droite. Bouton Continue violet fixé en bas dans un léger fondu blanc.

---

## 7.14 Consentement analytics

- overlay clair à 72 % ;
- blur `6 px` ;
- carte centrée maximum `420 px` ;
- rayon `18 px` ;
- ombre large à 8 % ;
- titre `20 px` ;
- texte `13 px`, ligne `1.55` ;
- deux actions de largeur égale.

Le langage visuel doit renforcer le caractère optionnel et rassurant. Ne pas utiliser d’icône d’alerte ou de couleur rouge.

---

## 7.15 Profil, plans et intégrations

### Carte de profil

- en-tête avec logo `26 px`, titre et badge de plan ;
- indicateur de moteur local dans une surface muted ;
- point de statut de `6 px` ;
- avatar de `28 px` ;
- nom et email tronqués proprement ;
- bouton Activate cloud ou Logout compact.

### Codes de plan

- Free : gris neutre ;
- Individual : bleu doux ;
- Team : vert doux ;
- aucun effet métallique ou premium doré.

### Intégrations

Les fournisseurs Jira et Linear apparaissent comme boutons secondaires côte à côte, avec leurs couleurs officielles uniquement dans leurs petites icônes.

### Prompt IA

> Vue de profil compacte composée de cartes blanches. En-tête avec petit logo, nom du produit et badge de plan pastel. Ligne d’état locale sur fond gris clair avec point vert. Profil utilisateur sur surface muted avec avatar rond, nom, email et petite action. Sections suivantes pour licence, préférences, confidentialité et intégrations, toutes sobres et uniformes.

---

## 7.16 Coach IA

### Layout

- conversation occupant toute la hauteur disponible ;
- messages scrollables ;
- compositeur fixé en bas de la vue ;
- prompts rapides sous le champ ;
- aucun grand header concurrent.

### Bulles

- largeur maximale `92%` ;
- rayon `14 px` ;
- utilisateur : fond violet, texte clair, coin inférieur droit `4 px` ;
- assistant : fond blanc, bordure claire, coin inférieur gauche `4 px` ;
- texte `12 px`, ligne `1.5`.

### Compositeur

- carte blanche séparée par une bordure supérieure ;
- champ interne sur fond global ;
- rayon `14 px` ;
- bouton Envoyer rond `32 px`, violet ;
- compteur `10 px` ;
- chips de prompt à `10 px`, pills bordées.

### État verrouillé

- conversation floutée de `6 px` ;
- overlay à faible opacité avec blur ;
- carte centrée de `280 px` ;
- CTA primaire.

### Prompt IA

> Chat de coaching minimaliste intégré à une application de productivité. Bulles utilisateur violettes alignées à droite, bulles assistant blanches bordées alignées à gauche, petits rayons avec un seul coin resserré. Compositeur blanc fixé en bas, champ arrondi et bouton d’envoi violet circulaire. Prompts rapides sous forme de petites pills. Aucun avatar ni dégradé de message.

---

## 7.17 Rapport de statut

### Intention

Le rapport mélange l’esthétique douce de Flowmates et une structure plus éditoriale de rapport professionnel.

### Modal

- largeur maximum `520 px` ;
- marge extérieure `12 px` ;
- hauteur maximum viewport moins `24 px` ;
- fond global clair ;
- rayon `20 px` ;
- overlay noir à 50 % avec blur `6 px` ;
- header blanc fixe ;
- body scrollable.

### Contenu

- titre centré ;
- métadonnées en petites boîtes ;
- synthèse sur fond bleu très pâle ;
- badge de santé large ;
- tableau compact à header ardoise foncé ;
- mini-barres hebdomadaires ;
- barres de catégories ;
- blocs risques en jaune pâle ;
- progrès en bleu pâle ;
- leçons sur fond violet pâle ;
- bouton PDF secondaire.

### Statuts

| Statut | Fond | Texte / contraste |
|---|---|---|
| On track | turquoise/vert | blanc ou vert sombre |
| Attention | jaune | brun sombre |
| At risk | rouge doux | blanc ou rouge sombre |

### Prompt IA

> Rapport professionnel compact dans une modal macOS arrondie. En-tête blanc fixe, corps sur fond blanc cassé. Titre centré, petites cartes de métadonnées, résumé exécutif bleu très pâle, grand badge de santé turquoise, tableau à en-tête ardoise, mini-histogrammes violets, blocs risques jaune pâle et progrès bleu pâle. Typographie SF Pro dense mais très lisible, nombreux rayons de 12 à 16 px, aucune décoration inutile.

---

## 7.18 Modals et overlays

### Modal générique

- fond overlay noir à 40 % ;
- blur `8 px` ;
- carte maximum `400 px` ;
- rayon `20 px` ;
- header séparé ;
- body scrollable ;
- ombre large douce.

### Overlay de setup local AI

- voile blanc à 85 % ;
- blur `16 px` ;
- carte blanche `360 px` maximum ;
- spinner de `32 px` ;
- barre de progression de `6 px` ;
- texte central rassurant.

Le setup utilise actuellement un gradient bleu-vers-vert dans la barre. Cette exception doit rester liée à un processus technique, pas devenir un motif de marque général.

---

## 7.19 Toasts

- position : bas droite ;
- maximum `280 px` ;
- fond blanc ;
- rayon global environ `20 px` ;
- padding `10 × 14 px` ;
- ombre `0 4px 12px / 15%` ;
- taille `12 px` ;
- bordure gauche `3 px` pour le statut ;
- entrée latérale en `300 ms`.

Succès : vert. Erreur : rouge. Un toast neutre ne doit pas inventer une couleur supplémentaire.

---

## 7.20 Badges, chips et points de statut

### Badge

- inline-flex ;
- texte `10–11 px` ;
- padding vertical `2–4 px` ;
- rayon capsule ;
- fond coloré à `12–18%` d’opacité ;
- texte de la même famille chromatique, plus sombre.

### Point de statut

- `6–8 px` ;
- halo optionnel à 20 % quand actif ;
- toujours accompagné d’un texte.

### Chip de prompt

- border gris clair ;
- fond clair ;
- texte secondaire ;
- hover lavande.

---

## 7.21 Scrollbar

- largeur `7 px` ;
- rail lavande `#EDE9FE` ;
- thumb violet `#8B5CF6` ;
- rayon capsule ;
- dégradé violet vertical discret ;
- bordure interne de `2 px` de la couleur du rail.

Cette scrollbar est plus expressive que le reste. Dans une adaptation très native à macOS, il est acceptable de laisser la scrollbar système, à condition de conserver la discrétion et de ne pas afficher un rail épais en permanence.

---

## 8. États esthétiques transversaux

| État | Traitement |
|---|---|
| Normal | fond clair, contraste modéré |
| Hover | lavande pâle ou augmentation légère d’ombre |
| Active / Selected | violet + lavande, éventuellement scale tactile |
| Focus clavier | contour violet net et halo faible, jamais supprimé |
| Disabled | opacité `0.45–0.60`, curseur non autorisé |
| Loading | spinner fin, texte d’état explicite |
| Empty | texte secondaire centré, beaucoup d’espace |
| Success | vert doux, jamais plein écran |
| Warning | orange ou jaune doux avec texte explicatif |
| Error | rouge limité au bord, au texte ou à une action ciblée |
| Locked | contenu flouté + carte explicative centrée |
| Offline local | neutre et rassurant, ne pas le traiter comme une panne |

---

## 9. Grammaire de description pour une autre IA

Pour obtenir un résultat fidèle, décrire toujours l’interface dans cet ordre :

1. **Plateforme et format** — macOS, fenêtre compacte, dimensions.
2. **But de l’écran** — suivi du temps, synthèse, profil, etc.
3. **Fond et matière** — blanc cassé, cartes blanches, ombres faibles.
4. **Hiérarchie** — quel composant domine.
5. **Composition** — ordre vertical et alignements.
6. **Palette** — neutres puis accent violet.
7. **Typographie** — SF Pro, chiffres tabulaires, tailles.
8. **Formes** — cercles, rayons, pills.
9. **États visibles** — actif, arrêté, vide, verrouillé.
10. **Contraintes négatives** — ce qui ne doit pas apparaître.

### 9.1 Formule de prompt

```text
Conçois [type d’écran] pour [plateforme + viewport].
L’utilisateur doit comprendre immédiatement [action ou donnée principale].

Direction visuelle : [3 à 6 adjectifs précis].
Fond : [couleur et matière].
Surface : [cartes, bordures, ombres, rayons].
Hiérarchie : [élément dominant], puis [éléments secondaires].
Typographie : [famille, tailles clés, poids].
Couleur d’action : [couleur + rôle].
États visibles : [liste].
Interactions : [hover, focus, animation].

Éviter : [liste de motifs incompatibles].
```

### 9.2 Vocabulaire précis au lieu de mots vagues

| Mot vague | Description utile |
|---|---|
| Moderne | SF Pro, fond `#FAFAFA`, cartes blanches, rayons `16–20 px`, ombres sous 10 % |
| Minimaliste | une action dominante, palette neutre, peu de séparateurs, pas d’ornement |
| Premium | proportions soignées, micro-contraste, mouvement court, pas de doré |
| Doux | grands rayons, bordures pâles, ombres diffuses, accent lavande |
| Natif macOS | typo système, densité compacte, interactions discrètes, fenêtre étroite redimensionnable |
| Data-driven | valeurs lisibles accompagnées de petits graphiques interprétables |
| Calme | grand espace négatif, faible saturation hors action, absence d’alertes décoratives |
| Tactile | boutons circulaires, pills, légère élévation et micro-scale à la pression |

---

## 10. Prompt maître pour transférer l’esthétique à un autre projet

```text
Crée une interface inspirée du langage visuel de Flowmates, sans reprendre son nom ni son contenu métier.

L’application doit ressembler à un produit macOS personnel, compact, calme et fiable. Utilise un fond blanc cassé #FAFAFA, des cartes blanches #FFFFFF, un texte principal bleu-noir #020817, un texte secondaire ardoise #64748B, des bordures #E2E8F0 et un violet #7C3BED comme unique couleur d’action principale. La plupart des surfaces ont des coins de 16 à 20 px et des ombres très faibles. Utilise SF Pro ou la police système Apple.

Construis une hiérarchie verticale simple avec une seule visualisation ou action dominante par écran. Favorise les anneaux de progression, les pastilles, les pills et les graphiques compacts. Les chiffres importants sont larges, gras et tabulaires. Les labels restent petits et gris. La navigation principale est fixe en bas avec icônes linéaires et libellés courts.

Les interactions sont discrètes : transitions de 150 à 250 ms, légère élévation au hover, scale très faible à la pression et focus clavier violet visible. Les états de succès, attention et erreur utilisent respectivement vert, orange/jaune et rouge, uniquement quand leur sens l’exige.

Le résultat doit évoquer la concentration, la progression personnelle et la confidentialité. Il ne doit pas ressembler à un dashboard financier, à une interface cyberpunk, à un produit de surveillance, à un jeu, ni à un panneau administratif dense. Pas de dark mode par défaut, pas de glassmorphism excessif, pas de gradients généralisés, pas d’ombres dures et pas de surcharge de métriques.
```

---

## 11. Prompts prêts à copier

### 11.1 Recréer le screenshot Today

```text
Génère un mockup UI haute fidélité d’une application macOS verticale de 370 × 700 px dédiée au suivi du temps. Fond #FAFAFA. En haut à gauche, petite salutation gris ardoise. Au centre supérieur, très grand anneau circulaire de progression, piste #F1F5F9 épaisse, arc violet #7C3BED à extrémités rondes. Dans le cercle : petit texte « Daily goal 6h », temps « 0:00 » en SF Pro 48 px bold avec chiffres tabulaires, puis bouton Play blanc circulaire de 56 px avec ombre douce. Sous l’anneau : « 0 days Streak » centré et gras. Ensuite, deux cartes blanches pleine largeur, coins 16 px, ombres très légères : sélecteur Daily goal et champ Current Task. En bas, navigation fixe blanche à quatre items Today, Summary, Pro, Profile, icônes linéaires ; Today est violet sur lavande pâle. Beaucoup d’espace vide, aucun logo dominant, aucune illustration.
```

### 11.2 Recréer la semaine-calendrier

```text
Crée un composant de continuité hebdomadaire très compact : sept colonnes uniformes, lettre du jour en 11 px gris au-dessus, cercle de 32 px en dessous. Jour inactif blanc bordé gris clair. Jour complété violet avec coche blanche. Jour actuel entouré de violet 2.5 px avec label foncé en gras. Une seule ligne, aucun mois, aucun événement, aucun quadrillage.
```

### 11.3 Recréer la roue temporelle

```text
Crée un composant radial responsive 1:1, diamètre visuel entre 300 et 500 px. SVG circulaire avec rayon relatif 82/100 et trait de 14/200. Piste gris bleuté très pâle, progression violette avec extrémités arrondies, départ à midi. Au centre, objectif en 12 px gris, durée en 48 px bold tabulaire, bouton Play blanc 56 px avec ombre douce. L’ensemble est minimal, sans ticks, sans aiguille et sans multiples anneaux.
```

### 11.4 Recréer la synthèse

```text
Conçois une vue Summary verticale pour une application de productivité macOS compacte. Titre 28 px, bouton Work report lavande en pill, date centrée grise, bande hebdomadaire de sept pastilles. Grande carte blanche 20 px radius avec anneau de progression 72 px, temps total 32 px et barres de répartition colorées. Section Highlights avec carte Deep Focus, phrase interprétative, mini histogramme turquoise de 8am à 8pm, puis carte de distractions et timeline fine. Fond #FAFAFA, ombres sous 6 %, violet #7C3BED.
```

### 11.5 Adapter à un nouveau domaine

```text
Transfère l’esthétique Flowmates à une application de [DOMAINE]. Remplace le suivi du temps par [MÉTRIQUE PRINCIPALE], mais conserve : une seule métrique dominante, un grand composant radial ou spatial, une palette neutre avec accent violet, des cartes blanches 16–20 px, la typographie système Apple, une navigation basse à quatre entrées et des états doux. Remplace les catégories et contenus sans modifier la hiérarchie visuelle fondamentale.
```

### 11.6 Prompt négatif générique

```text
Éviter les fonds sombres, le néon, les gradients multiples, les ombres noires lourdes, les bordures épaisses, les cartes imbriquées sans fin, les tableaux denses, les jauges de type automobile, les graphiques 3D, les effets verre excessifs, les couleurs de statut décoratives, les gros logos, les illustrations mascottes, les animations rebondissantes et la gamification enfantine.
```

---

## 12. Comment appliquer ce style à d’autres projets

### 12.1 Invariants à conserver

1. Fond blanc cassé et cartes blanches.
2. Accent violet unique pour l’action et la progression.
3. Typographie système Apple ou équivalent sobre.
4. Rayons généreux de `16–20 px`.
5. Ombres sous 10 % pour les surfaces courantes.
6. Une seule priorité visuelle par écran.
7. Données accompagnées de texte humain.
8. Navigation courte, stable et iconographique.
9. États discrets mais explicites.
10. Grand usage de l’espace négatif.

### 12.2 Variables personnalisables

- le violet peut devenir une couleur de marque différente si elle garde une luminosité et une saturation comparables ;
- l’anneau peut devenir une carte spatiale, un compteur ou une illustration de progression ;
- la navigation peut passer à un rail latéral sur écran large ;
- les catégories de graphiques changent selon le métier ;
- la bande hebdomadaire peut représenter une autre cadence ;
- les emojis de l’onboarding peuvent être remplacés par de petites icônes.

### 12.3 Ce qui casserait l’identité

- remplir l’espace autour de l’anneau ;
- transformer toutes les cartes en panneaux bordés ;
- utiliser le violet pour tout le texte ;
- afficher plus de six couleurs de données à la fois ;
- mettre plusieurs CTA primaires côte à côte ;
- rendre la navigation et le contenu également dominants ;
- augmenter fortement la densité sans créer de niveaux de lecture ;
- utiliser une largeur desktop complète sans rail central.

### 12.4 Adaptation à un écran large

Pour un écran supérieur à 900 px :

- conserver un rail principal de `420–560 px` ;
- placer les détails secondaires dans un panneau latéral ;
- transformer la navigation basse en rail latéral étroit si nécessaire ;
- ne pas agrandir l’anneau au-delà de `500 px` ;
- préserver les espacements internes et la taille de lecture.

### 12.5 Adaptation mobile

- conserver la navigation basse ;
- passer le padding horizontal de `16 px` à `14–16 px` ;
- maintenir des cibles tactiles d’au moins `44 px` même si l’icône est plus petite ;
- garder l’anneau entre `calc(100vw - 32px)` et `360 px` ;
- empiler tous les blocs de rapport.

### 12.6 Adaptation dark mode éventuelle

Le produit audité est clair uniquement. Si un dark mode est créé :

- utiliser un bleu-noir légèrement levé, pas `#000` ;
- cartes seulement 3–6 % plus claires que le fond ;
- réduire les ombres et augmenter les bordures ;
- désaturer légèrement le violet ;
- conserver le contraste des graphiques ;
- tester les états pastel, qui ne peuvent pas être simplement inversés.

---

## 13. Accessibilité à intégrer dans toute reproduction

La fidélité esthétique ne doit pas reproduire les limites éventuelles du code source.

### Exigences

- focus clavier toujours visible ;
- contraste AA pour texte et icônes ;
- corps de texte idéalement `12 px` minimum dans une fenêtre desktop, plus grand sur mobile ;
- cible interactive minimum `44 × 44 px` pour les actions principales ;
- ne pas transmettre un statut uniquement par couleur ;
- ajouter libellés ou textes accessibles aux icônes ;
- respecter `prefers-reduced-motion` ;
- annoncer les changements du timer et les erreurs sans lecture continue intrusive ;
- fournir une alternative textuelle aux graphiques ;
- ne pas flouter un contenu verrouillé sans expliquer comment le déverrouiller ;
- garantir la lisibilité à `340 × 400 px`.

### Focus recommandé

```css
:focus-visible {
  outline: 2px solid #7c3bed;
  outline-offset: 2px;
  box-shadow: 0 0 0 4px rgb(124 59 237 / 0.16);
}
```

---

## 14. Checklist de validation esthétique

### Vue globale

- [ ] Le fond est blanc cassé, pas blanc pur partout.
- [ ] Une seule action ou donnée domine l’écran.
- [ ] Le contenu respire autour du composant principal.
- [ ] La largeur de lecture reste contenue.
- [ ] La navigation ne prend pas plus d’importance que le contenu.

### Couleurs

- [ ] Le violet indique action, progression ou sélection.
- [ ] Les couleurs de statut sont sémantiques.
- [ ] Les graphiques utilisent six couleurs maximum.
- [ ] Le texte principal est bleu-noir, pas gris clair.
- [ ] Les fonds pastel restent très peu saturés.

### Formes et matières

- [ ] Les cartes ont des rayons cohérents de `16–20 px`.
- [ ] Les ombres sont diffuses et faibles.
- [ ] Les pills sont utilisées pour de petits statuts ou actions.
- [ ] Les bordures ne sont pas trop nombreuses.
- [ ] Les composants circulaires ont une fonction temporelle ou de progression.

### Typographie

- [ ] Les chiffres temporels sont tabulaires.
- [ ] Les titres ont un léger tracking négatif.
- [ ] Les labels sont courts et secondaires.
- [ ] L’uppercase est limité aux métadonnées.
- [ ] Aucun texte essentiel n’est minuscule ou peu contrasté.

### Interaction

- [ ] Le hover reste discret.
- [ ] Le focus clavier est violet et visible.
- [ ] Les transitions durent moins de 500 ms.
- [ ] Le loading indique ce qui se passe.
- [ ] L’état disabled reste compréhensible.

### Anneau temporel

- [ ] Il est le composant dominant de Today.
- [ ] La progression commence à midi.
- [ ] L’arc a des extrémités arrondies.
- [ ] La durée est la valeur la plus lisible.
- [ ] Le bouton Play reste un objet distinct au centre.
- [ ] Il n’y a ni ticks, ni aiguille, ni anneaux multiples.

### Calendrier hebdomadaire

- [ ] Exactement sept jours sur une ligne.
- [ ] Les jours complétés ont une coche.
- [ ] Aujourd’hui est distingué même sans activité.
- [ ] Le composant ne ressemble pas à un date picker.

---

## 15. Carte des composants vers les sources actuelles

| Composant | Source principale |
|---|---|
| Tokens, contrôles, cartes | `apps/agent/src/renderer/styles.css` |
| Structure Today | `apps/agent/src/renderer/index.html` |
| Anneau temporel | `index.html`, classes `.today-timer-*` |
| Semaine-calendrier | `app.js::renderWeekRow`, classes `.week-*` |
| Résumé | `app.js::renderHistory`, classes `.summary-*` |
| Barres de tâches | `app.js::renderTaskBars`, classes `.task-bar-*` |
| Focus chart | `app.js::renderFocusChart`, classes `.focus-*` |
| Timeline | `app.js::renderTimeline`, classes `.timeline-*` |
| Login | `index.html`, classes `.login-splash-*` |
| Onboarding | `app.js::renderOnboardingStep`, classes `.onboarding-*` |
| Coach | `index.html`, classes `.coach-*` |
| Rapport | `app.js::renderStatusReportPanel`, classes `.sr-*` |
| Fenêtre macOS | `apps/agent/src-tauri/tauri.conf.json` |

---

## 16. Résumé opérationnel

Pour reproduire Flowmates dans un autre produit, retenir ce noyau :

> Concevoir une petite fenêtre de productivité calme sur fond `#FAFAFA`, centrée autour d’une seule action ou métrique forte. Utiliser des cartes blanches arrondies, des ombres presque invisibles, du texte bleu-noir, des labels ardoise et un violet franc pour l’action et la progression. Donner aux données des formes circulaires, des pills et de petits graphiques faciles à lire. Garder la navigation stable, les animations courtes et les états rassurants. L’interface doit donner le sentiment que l’utilisateur comprend son travail, pas qu’il est surveillé.

