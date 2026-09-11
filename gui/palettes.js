/* **cian の配色の写し**（依頼 495）。手で直さない ──
 *
 *     node scripts/themes-test.js --write
 *
 * で隣の cian（`crates/cian-core/src/theme.rs` と `gui/index.html`）から作り直す。
 * 本人が決めた: 「全テーマを全く同一に合わせたい」。amber は cian を知らない
 * （依存は cian → amber の一方向）ので、表を写して持つ。写しが古くなれば
 * `themes-test` が鳴る。
 */
'use strict';

/// 十八の配色。並び順は cian の `:theme` と同じ。
const CIAN_PALETTES = [
    { name: "solarized-light", bg: "#fdf6e3", fg: "#657b83", dim: "#93a1a1", border: "#93a1a1", accent: "#268bd2", sel: "#dcd5be", visual: "#f7e4b0", mark: "#cb4b16", popup: "#f5efdc", status: "#eee8d5", blue: "#268bd2", yellow: "#b58900", cyan: "#2aa198", magenta: "#d33682", red: "#dc322f", green: "#859900", doc: "#586e75" },
    { name: "solarized-dark", bg: "#002b36", fg: "#839496", dim: "#586e75", border: "#586e75", accent: "#268bd2", sel: "#073642", visual: "#0a4a5a", mark: "#cb4b16", popup: "#073642", status: "#073642", blue: "#268bd2", yellow: "#b58900", cyan: "#2aa198", magenta: "#d33682", red: "#dc322f", green: "#859900", doc: "#93a1a1" },
    { name: "dracula", bg: "#282a36", fg: "#f8f8f2", dim: "#6272a4", border: "#6272a4", accent: "#bd93f9", sel: "#44475a", visual: "#424458", mark: "#ffb86c", popup: "#21222c", status: "#191a21", blue: "#bd93f9", yellow: "#f1fa8c", cyan: "#8be9fd", magenta: "#ff79c6", red: "#ff5555", green: "#50fa7b", doc: "#f8f8f2" },
    { name: "nord", bg: "#2e3440", fg: "#d8dee9", dim: "#4c566a", border: "#4c566a", accent: "#88c0d0", sel: "#3b4252", visual: "#434c5e", mark: "#ebcb8b", popup: "#272c36", status: "#3b4252", blue: "#81a1c1", yellow: "#ebcb8b", cyan: "#88c0d0", magenta: "#b48ead", red: "#bf616a", green: "#a3be8c", doc: "#e5e9f0" },
    { name: "gruvbox-dark", bg: "#282828", fg: "#ebdbb2", dim: "#928374", border: "#504945", accent: "#fe8019", sel: "#3c3836", visual: "#504945", mark: "#fabd2f", popup: "#1d2021", status: "#3c3836", blue: "#83a598", yellow: "#fabd2f", cyan: "#8ec07c", magenta: "#d3869b", red: "#fb4934", green: "#b8bb26", doc: "#ebdbb2" },
    { name: "gruvbox-light", bg: "#fbf1c7", fg: "#3c3836", dim: "#7c6f64", border: "#d5c4a1", accent: "#af3a03", sel: "#ebdbb2", visual: "#d5c4a1", mark: "#b57614", popup: "#f2e5bc", status: "#ebdbb2", blue: "#076678", yellow: "#b57614", cyan: "#427b58", magenta: "#8f3f71", red: "#9d0006", green: "#79740e", doc: "#3c3836" },
    { name: "tokyo-night", bg: "#1a1b26", fg: "#c0caf5", dim: "#565f89", border: "#292e42", accent: "#7aa2f7", sel: "#292e42", visual: "#33467c", mark: "#e0af68", popup: "#16161e", status: "#16161e", blue: "#7aa2f7", yellow: "#e0af68", cyan: "#7dcfff", magenta: "#bb9af7", red: "#f7768e", green: "#9ece6a", doc: "#c0caf5" },
    { name: "catppuccin-mocha", bg: "#1e1e2e", fg: "#cdd6f4", dim: "#6c7086", border: "#313244", accent: "#89b4fa", sel: "#313244", visual: "#45475a", mark: "#f9e2af", popup: "#181825", status: "#181825", blue: "#89b4fa", yellow: "#f9e2af", cyan: "#94e2d5", magenta: "#f5c2e7", red: "#f38ba8", green: "#a6e3a1", doc: "#cdd6f4" },
    { name: "catppuccin-latte", bg: "#eff1f5", fg: "#4c4f69", dim: "#6c6f85", border: "#ccd0da", accent: "#1e66f5", sel: "#ccd0da", visual: "#dce0e8", mark: "#df8e1d", popup: "#e6e9ef", status: "#ccd0da", blue: "#1e66f5", yellow: "#df8e1d", cyan: "#179299", magenta: "#ea76cb", red: "#d20f39", green: "#40a02b", doc: "#4c4f69" },
    { name: "monokai", bg: "#272822", fg: "#f8f8f2", dim: "#75715e", border: "#3e3d32", accent: "#66d9ef", sel: "#3e3d32", visual: "#49483e", mark: "#fd971f", popup: "#1e1f1c", status: "#3e3d32", blue: "#66d9ef", yellow: "#e6db74", cyan: "#66d9ef", magenta: "#ae81ff", red: "#f92672", green: "#a6e22e", doc: "#f8f8f2" },
    { name: "one-dark", bg: "#282c34", fg: "#abb2bf", dim: "#5c6370", border: "#3b4048", accent: "#61afef", sel: "#3b4048", visual: "#3e4451", mark: "#e5c07b", popup: "#21252b", status: "#21252b", blue: "#61afef", yellow: "#e5c07b", cyan: "#56b6c2", magenta: "#c678dd", red: "#e06c75", green: "#98c379", doc: "#abb2bf" },
    { name: "github-light", bg: "#ffffff", fg: "#24292e", dim: "#6a737d", border: "#d1d5da", accent: "#0366d6", sel: "#eef2f5", visual: "#dbe9ff", mark: "#e36209", popup: "#f6f8fa", status: "#eaeef2", blue: "#0366d6", yellow: "#b08800", cyan: "#1b7c83", magenta: "#6f42c1", red: "#d73a49", green: "#22863a", doc: "#24292e" },
    { name: "monokai-pro", bg: "#2d2a2e", fg: "#fcfcfa", dim: "#727072", border: "#5b595c", accent: "#ffd866", sel: "#423f42", visual: "#5b595c", mark: "#fc9867", popup: "#221f22", status: "#221f22", blue: "#78dce8", yellow: "#ffd866", cyan: "#78dce8", magenta: "#ab9df2", red: "#ff6188", green: "#a9dc76", doc: "#c1c0c0" },
    { name: "ayu-dark", bg: "#0d1017", fg: "#bfbdb6", dim: "#565b66", border: "#1d2229", accent: "#e6b450", sel: "#1d2733", visual: "#2d3640", mark: "#ff8f40", popup: "#131721", status: "#11151c", blue: "#59c2ff", yellow: "#e6b450", cyan: "#95e6cb", magenta: "#d2a6ff", red: "#f26d78", green: "#aad94c", doc: "#acb6bf" },
    { name: "ayu-light", bg: "#fcfcfc", fg: "#5c6166", dim: "#8a9199", border: "#e7e8e9", accent: "#f2ae49", sel: "#eaeaeb", visual: "#ffe9b3", mark: "#fa8d3e", popup: "#f3f3f3", status: "#f0f0f0", blue: "#399ee6", yellow: "#f2ae49", cyan: "#4cbf99", magenta: "#a37acc", red: "#f07171", green: "#86b300", doc: "#787b80" },
    { name: "bluloco-light", bg: "#f9f9f9", fg: "#383a42", dim: "#a0a1a7", border: "#d4d4d4", accent: "#275fe4", sel: "#e5e5e6", visual: "#d7e0f5", mark: "#d52753", popup: "#f0f0f0", status: "#efefef", blue: "#275fe4", yellow: "#c18401", cyan: "#0098dd", magenta: "#823ff1", red: "#d52753", green: "#23974a", doc: "#7a82da" },
    { name: "bearded", bg: "#16161d", fg: "#ebebf0", dim: "#6c6f93", border: "#2a2a3c", accent: "#a45fff", sel: "#2c2c3f", visual: "#3a2f55", mark: "#ff3e7b", popup: "#1d1d28", status: "#1d1d28", blue: "#50b0f0", yellow: "#ffb86c", cyan: "#21c7a8", magenta: "#ff3e7b", red: "#ff5f87", green: "#7ddb8a", doc: "#b9bacb" },
    { name: "finder", bg: "#ffffff", fg: "#1d1d1f", dim: "#86868b", border: "#d8d8dc", accent: "#0a84ff", sel: "#0a84ff", visual: "#d6e9ff", mark: "#ff9500", popup: "#f7f7f9", status: "#ececee", blue: "#2f7de0", yellow: "#9a6b00", cyan: "#0a7f8c", magenta: "#a63aa6", red: "#c0392b", green: "#2f8a3e", doc: "#3a3a3c" },
];

/// 窓の三つの装い（白磁・陰翳・端末譲り）── cian の `index.html` の変数そのまま。
const CIAN_LOOKS = {
    hakuji: { "bg": "#f7f8f8", "pane": "#ffffff", "pane-off": "#f3f5f5", "line": "#e3e7e6", "text": "#2b3333", "dim": "#8b9493", "dir": "#17706a", "accent": "#0e9e8f", "accent-dim": "#e6f1ef", "on-accent": "#ffffff", "sel-strong": "#c8e4df", "row-hover": "#eef2f1", "mark": "#bf6b34" },
    inei: { "bg": "#14110f", "pane": "#1c1714", "pane-off": "#161211", "line": "#2b2320", "text": "#d9d0c5", "dim": "#7c6f64", "dir": "#c9a227", "accent": "#e8b84b", "accent-dim": "#2e2519", "on-accent": "#1a1409", "sel-strong": "#3d3120", "row-hover": "#241d19", "mark": "#c8703c" },
    terminal: { "bg": "#0c0c0c", "pane": "#111111", "pane-off": "#0d0d0d", "line": "#232323", "text": "#c8c8c8", "dim": "#6a6a6a", "dir": "#5fafff", "accent": "#5fffd7", "accent-dim": "#1c2a28", "on-accent": "#04120f", "sel-strong": "#263a37", "row-hover": "#1a1a1a", "mark": "#ffd75f" },
};
