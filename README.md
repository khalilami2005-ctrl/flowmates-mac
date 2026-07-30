# Flowmates — macOS

Agent de mesure d'activité pour macOS, en Rust + Tauri 2. Il observe le contexte
de travail, le fait analyser par un modèle de vision **local**, et produit des
rapports. Rien de ce qui décrit l'écran ne quitte la machine.

C'est la déclinaison macOS de Flowmates. La version Windows vit dans un dépôt
séparé et partage la marque, pas le code.

---

## Ce qui tourne en local, et ce qui n'existe pas encore

Le moteur d'inférence (`llama.cpp`, accéléré par Metal) et les poids sont
embarqués dans l'application. Il n'y a pas de téléchargement au premier
lancement, et pas d'aller-retour réseau pour analyser un écran.

**Aucune adresse de serveur n'est compilée dans ce binaire.** Une construction
livrée sans configuration n'a pas de cloud : la mesure locale, l'analyse locale
et les rapports locaux fonctionnent ; l'authentification, la synchronisation et
les intégrations se déclarent indisponibles au lieu de contacter un hôte. Voir
`apps/agent/src-tauri/src/sync_env.rs` — le test `no_host_is_compiled_in` est là
pour que ça le reste.

Renseigner `NEXT_PUBLIC_SUPABASE_URL` et `NEXT_PUBLIC_SUPABASE_ANON_KEY` dans
`.env.local` active ces fonctions. Le backend correspondant est à construire :
il n'est pas dans ce dépôt.

---

## Démarrage

### Prérequis

- **macOS 14 ou plus récent**, Apple Silicon ou Intel
- Outils en ligne de commande Xcode : `xcode-select --install`
- **Rust 1.77.2** ou plus récent
- **Node.js 20.19** ou plus récent, **pnpm 8.15**

### Lancer

```bash
git clone https://github.com/khalilami2005-ctrl/flowmates-mac.git
cd flowmates-mac
pnpm install
pnpm exec node scripts/fetch-models.mjs --check   # vérifie moteur et poids
pnpm dev
```

Les poids sont déjà présents dans `local_llm/`. Le script vérifie leur taille
exacte et leur empreinte SHA-256, ainsi que le mode exécutable et les tranches
`arm64` + `x86_64` de `llama-server` ; il ne télécharge que ce qui manque.

### Autorisations système à accorder au premier lancement

Réglages → Confidentialité et sécurité :

- **Enregistrement de l'écran** — sans elle, aucune analyse d'écran
- **Accessibilité** — sans elle, le contexte de fenêtre reste vide

### Construire

```bash
pnpm build                                        # .app et .dmg, architecture native
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build --target universal-apple-darwin  # binaire universel, comme la CI
```

Les artefacts atterrissent dans
`apps/agent/src-tauri/target/release/bundle/`. La signature Developer ID et la
notarisation sont décrites dans [`docs/macos-release.md`](./docs/macos-release.md).

`createUpdaterArtifacts` est actif : la construction signe les artefacts de mise
à jour et réclame la clé privée. Sa contrepartie publique est dans
`tauri.conf.json`, et **sa perte est définitive** — aucune copie installée
n'accepterait plus jamais de mise à jour.

---

## Architecture

```text
apps/agent/
  src-tauri/       application Tauri, 21 fichiers Rust
  src/renderer/    interface HTML/CSS/JS
local_llm/
  bin/             llama-server macOS
  *.gguf           poids du modèle de vision (hors dépôt)
scripts/
  fetch-models.mjs vérifie et récupère moteur et poids
docs/
  macos-release.md   publication : signature, notarisation, Gatekeeper
  visual-language.md langage visuel complet de l'interface
  resource-analysis.md consommation mesurée
tools/
  popular-apps-catalog/ catalogue d'applications pour la classification
```

Le traitement lourd — résumé de contexte, filtrage, inférence d'intention —
passe par le `llama-server` embarqué. Seuls des agrégats déjà filtrés peuvent
atteindre un backend, et uniquement si l'utilisateur en a configuré un et
rejoint une équipe.

---

## Vérifications

```bash
pnpm test                                  # tests Rust
pnpm check                                 # frontend + clippy, ce qu'exige la CI
pnpm exec node scripts/fetch-models.mjs --check
```

---

## Licence

Aucune licence n'est publiée pour l'instant. Le droit d'auteur s'applique seul :
**tous droits réservés**. Le dépôt est public — le code est donc lisible, mais
ni réutilisable ni redistribuable sans accord écrit.

Les licences des dépendances tierces restent dues et sont dans
[`THIRD_PARTY_NOTICES.md`](./THIRD_PARTY_NOTICES.md).
