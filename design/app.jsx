// LuxePix shell components
const { useState } = React;
const Icon = window.LpIcon;

// Picsum-style stable colored thumbnail (no network)
const Thumb = ({ seed, hue, sat = 60, light = 55, alt = "" }) => {
  // Generate a calm gradient placeholder; varied per seed
  const h1 = hue ?? ((seed * 47) % 360);
  const h2 = (h1 + 30) % 360;
  return (
    <div style={{
      width: "100%", height: "100%",
      background: `linear-gradient(135deg, hsl(${h1} ${sat}% ${light}%), hsl(${h2} ${sat - 15}% ${light - 15}%))`,
      position: "relative",
    }} aria-label={alt}>
      <div style={{
        position: "absolute", inset: 0,
        background: `radial-gradient(circle at 30% 30%, rgba(255,255,255,0.18), transparent 60%)`,
      }} />
    </div>
  );
};

const Slider = ({ value = 0.5 }) => (
  <div className="lp-slider">
    <Icon name="image" s={12} />
    <div className="lp-slider-track">
      <div className="lp-slider-fill" style={{ width: `${value * 100}%` }} />
      <div className="lp-slider-thumb" style={{ left: `${value * 100}%` }} />
    </div>
    <Icon name="image" s={16} />
  </div>
);

const TopBar = ({ path, theme, onToggleTheme, historyOpen }) => (
  <div className="lp-topbar">
    <div className="lp-brand">
      <div className="lp-brand-mark">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
             strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="5" width="18" height="14" rx="2.5" />
          <circle cx="9" cy="11" r="1.6" fill="currentColor" />
          <path d="m4 17 5-4 4 3 3-2 4 3" />
        </svg>
      </div>
      <div className="lp-brand-name">LuxePix</div>
    </div>
    <div className="lp-divider-v" />

    <div className="lp-pathfield">
      <div className="lp-input-icon"><Icon name="folder-open" s={14} /></div>
      <div className="lp-input">
        <div className="lp-path-text">
          {path
            ? <>D:<span className="lp-path-sep">/</span>Photography<span className="lp-path-sep">/</span>2026<span className="lp-path-sep">/</span><span style={{fontWeight:550}}>Client Shoots</span></>
            : <span className="lp-path-dim">Paste or type a folder path…</span>}
        </div>
      </div>
    </div>

    <button className="lp-iconbtn" title="Browse for folder…">
      <Icon name="folder-open" s={16} />
      <span className="lp-iconbtn-label">Browse</span>
    </button>
    <button className={"lp-iconbtn" + (historyOpen ? " is-active" : "")} title="Recent paths">
      <Icon name="history" s={16} />
    </button>
    <button className="lp-iconbtn" title="Refresh">
      <Icon name="refresh" s={16} />
    </button>

    <div style={{ flex: 1 }} />

    <button className="lp-iconbtn" title={theme === "dark" ? "Switch to light" : "Switch to dark"} onClick={onToggleTheme}>
      <Icon name={theme === "dark" ? "sun" : "moon"} s={16} />
    </button>
  </div>
);

const FOLDERS = [
  { name: "Client Shoots", images: 248, files: 252, hue: 20, selected: true },
  { name: "Portraits", images: 162, files: 164, hue: 340 },
  { name: "Landscapes", images: 94, files: 94, hue: 200 },
  { name: "Product · Loom", images: 78, files: 82, hue: 50 },
  { name: "Product · Vase", images: 56, files: 56, hue: 280 },
  { name: "Travel · Hà Giang", images: 312, files: 320, hue: 140 },
  { name: "Travel · Đà Lạt", images: 188, files: 188, hue: 100 },
  { name: "Studio Tests", images: 0, files: 6, hue: 0, empty: true },
  { name: "Archive 2025", images: 1428, files: 1450, hue: 230 },
];

const FolderTile = ({ f, selected, focused, renaming }) => (
  <div className={"lp-folder-tile" + (selected ? " is-selected" : "") + (focused ? " is-focused" : "")}>
    <div className={"lp-folder-thumb" + (f.empty ? " is-empty" : "")}>
      {f.empty ? <Icon name="folder" s={28} /> : <Thumb seed={f.name.length} hue={f.hue} />}
    </div>
    <div className="lp-folder-info">
      {renaming ? (
        <div className="lp-rename">
          {f.name.replace(/$/, "")}<span className="lp-rename-caret" />
        </div>
      ) : (
        <div className="lp-folder-name">{f.name}</div>
      )}
      <div className="lp-folder-counts">
        <span className="lp-count-pill"><Icon name="image" s={10} />{f.images.toLocaleString()}</span>
        <span style={{ color: "var(--text-tertiary)", fontSize: 11 }}>{f.files} files</span>
      </div>
    </div>
  </div>
);

const FolderPaneInner = ({ renamingId }) => (
  <React.Fragment>
    <div className="lp-pane-header">
      <div>
        <div className="lp-pane-title">Folders</div>
        <div className="lp-pane-meta lp-num">9 folders <span className="lp-meta-dot" /> 2,566 images</div>
      </div>
      <div className="lp-pane-spacer" />
      <Slider value={0.42} />
    </div>
    <div className="lp-pane-scroll">
      <div className="lp-folder-grid">
        {FOLDERS.map((f, i) => (
          <FolderTile key={f.name} f={f} selected={f.selected} renaming={renamingId === i} />
        ))}
      </div>
    </div>
  </React.Fragment>
);

const FolderPane = ({ renamingId }) => (
  <div className="lp-folder-pane">
    <FolderPaneInner renamingId={renamingId} />
  </div>
);

// Files
const IMAGES = [
  { name: "DSC_4821.NEF.jpg", w: 6000, h: 4000, kb: 4820, hue: 18 },
  { name: "DSC_4822.NEF.jpg", w: 6000, h: 4000, kb: 4710, hue: 22, focused: true },
  { name: "DSC_4823.NEF.jpg", w: 6000, h: 4000, kb: 4665, hue: 26, selected: true },
  { name: "DSC_4824.NEF.jpg", w: 6000, h: 4000, kb: 4920, hue: 30 },
  { name: "DSC_4825.NEF.jpg", w: 6000, h: 4000, kb: 4445, hue: 34 },
  { name: "DSC_4826.NEF.jpg", w: 6000, h: 4000, kb: 4815, hue: 38 },
  { name: "BTS_handheld_01.jpg", w: 4000, h: 3000, kb: 2210, hue: 42 },
  { name: "BTS_handheld_02.jpg", w: 4000, h: 3000, kb: 2080, hue: 200 },
  { name: "BTS_handheld_03.jpg", w: 4000, h: 3000, kb: 2120, loading: true },
  { name: "BTS_handheld_04.jpg", w: 4000, h: 3000, kb: 0, broken: true },
  { name: "rooftop_dusk.jpg", w: 5400, h: 3600, kb: 3990, hue: 280 },
  { name: "rooftop_dusk_2.jpg", w: 5400, h: 3600, kb: 4015, hue: 295 },
];
const DOCS = [
  { name: "shotlist_v3.pdf", ext: "PDF", kb: 184, kind: "doc" },
  { name: "release_form.docx", ext: "DOCX", kb: 42, kind: "doc" },
  { name: "color_grade.lut", ext: "LUT", kb: 12, kind: "doc" },
  { name: "raw_b-roll.mp4", ext: "MP4", kb: 248_000, kind: "video" },
];

const fmtSize = (kb) => kb >= 1024 ? `${(kb/1024).toFixed(1)} MB` : `${kb} KB`;

const FileTile = ({ file, renaming }) => {
  const isImg = !file.kind;
  return (
    <div className={"lp-file-tile" + (file.selected ? " is-selected" : "") + (file.focused ? " is-focused" : "")}>
      <div className={"lp-file-thumb" + (file.kind ? " is-doc" : "") + (file.loading ? " is-loading" : "") + (file.broken ? " is-broken" : "")}>
        {file.loading && <div className="lp-skeleton" />}
        {file.broken && (<><Icon name="broken" s={22} /><span>Couldn't load</span></>)}
        {!file.loading && !file.broken && isImg && <Thumb seed={file.name.length * 3} hue={file.hue} />}
        {file.kind === "doc" && (<><Icon name="doc" s={28} /><div className="lp-doc-ext">{file.ext}</div></>)}
        {file.kind === "video" && (<><Icon name="video" s={28} /><div className="lp-doc-ext">{file.ext}</div></>)}
      </div>
      <div className="lp-file-info">
        {renaming ? (
          <div className="lp-rename">
            DSC_4823_final<span className="lp-rename-caret" />
          </div>
        ) : (
          <div className="lp-file-name">{file.name}</div>
        )}
        <div className="lp-file-meta lp-num">
          {isImg ? <>{file.w.toLocaleString()}×{file.h.toLocaleString()} · {fmtSize(file.kb)}</>
                 : <>{file.ext} · {fmtSize(file.kb)}</>}
        </div>
      </div>
    </div>
  );
};

const ContentPane = ({ folder = "Client Shoots", renaming = -1 }) => (
  <div className="lp-content-pane">
    <div className="lp-pane-header">
      <div>
        <div className="lp-pane-title">{folder}</div>
        <div className="lp-pane-meta lp-num">248 images <span className="lp-meta-dot" /> 4 other files</div>
      </div>
      <div className="lp-pane-spacer" />
      <Slider value={0.58} />
    </div>
    <div className="lp-content-scroll">
      <div className="lp-section-label">Images <span className="lp-section-count lp-num">· 12 of 248</span></div>
      <div className="lp-file-grid">
        {IMAGES.map((f, i) => <FileTile key={f.name} file={f} renaming={renaming === i} />)}
      </div>
      <div className="lp-section-label">Other files <span className="lp-section-count lp-num">· 4</span></div>
      <div className="lp-file-grid">
        {DOCS.map((f) => <FileTile key={f.name} file={f} />)}
      </div>
    </div>
  </div>
);

const StatusBar = ({ kind = "info", message = "Scanned 248 images and 4 other files in Client Shoots · 1.2s" }) => (
  <div className="lp-statusbar">
    <span className={"lp-status-dot is-" + kind} />
    <span>{message}</span>
    <span className="lp-status-spacer" />
    <span className="lp-num" style={{ color: "var(--text-tertiary)" }}>D:/Photography/2026/Client Shoots</span>
  </div>
);

// History dropdown popover
const RECENTS = [
  { path: "D:/Photography/2026/Client Shoots", meta: "Today" },
  { path: "D:/Photography/2026/Portraits", meta: "Today" },
  { path: "D:/Photography/2025/Travel · Hà Giang", meta: "Yesterday" },
  { path: "E:/Archive/2024/Wedding · Linh & An", meta: "3 days ago" },
  { path: "D:/Stock/Product · Loom", meta: "Last week" },
  { path: "C:/Users/mai/Pictures/Screenshots", meta: "Last week" },
];

const HistoryPopover = ({ style }) => (
  <div className="lp-popover" style={style}>
    <div className="lp-popover-header">
      Recent paths
      <span className="lp-popover-clear">Clear all</span>
    </div>
    <div className="lp-popover-list">
      {RECENTS.map((r, i) => (
        <div key={r.path} className="lp-recent-item">
          <Icon name={i === 0 ? "folder-open" : "folder"} s={14} className="lp-recent-icon" />
          <span className="lp-recent-path lp-num">{r.path}</span>
          <span className="lp-recent-meta">{r.meta}</span>
          <button className="lp-recent-remove" title="Remove from history">
            <Icon name="x-small" s={12} />
          </button>
        </div>
      ))}
    </div>
  </div>
);

// Lightbox
const Lightbox = ({ loading, first, last, idx = 47, total = 248 }) => (
  <div className="lp-lightbox">
    <button className="lp-lightbox-close" title="Close (Esc)">
      <Icon name="close" s={18} />
    </button>
    <button className={"lp-lightbox-nav is-prev" + (first ? " is-disabled" : "")} title="Previous (←)">
      <Icon name="chevron-left" s={24} />
    </button>
    <button className={"lp-lightbox-nav is-next" + (last ? " is-disabled" : "")} title="Next (→)">
      <Icon name="chevron-right" s={24} />
    </button>
    <div className="lp-lightbox-stage">
      {loading ? (
        <div className="lp-lightbox-loading">
          <div className="lp-spinner" />
          <div>Loading DSC_4823.NEF.jpg…</div>
        </div>
      ) : (
        <div style={{
          width: "min(86%, 1100px)",
          aspectRatio: "3 / 2",
          borderRadius: 4,
          background: "linear-gradient(135deg, hsl(28 55% 55%), hsl(12 50% 35%))",
          position: "relative",
          boxShadow: "0 20px 60px rgba(0,0,0,0.5)",
        }}>
          <div style={{
            position: "absolute", inset: 0,
            background: "radial-gradient(circle at 35% 30%, rgba(255,255,255,0.18), transparent 55%), radial-gradient(circle at 75% 80%, rgba(0,0,0,0.25), transparent 50%)",
          }} />
        </div>
      )}
    </div>
    <div className="lp-lightbox-info">
      <span className="lp-li-filename">DSC_4823.NEF.jpg</span>
      <span className="lp-li-meta">6000 × 4000  ·  4.6 MB</span>
      <span className="lp-li-spacer" />
      <span className="lp-li-zoom">Fit · 38%</span>
      <span className="lp-li-counter">{idx} / {total}</span>
    </div>
  </div>
);

// Empty / initial state
const InitialState = ({ theme }) => (
  <div className="lp-app" data-theme={theme} style={{ width: "100%", height: "100%" }}>
    <TopBar path={null} theme={theme} />
    <div className="lp-empty">
      <div className="lp-empty-art">
        <Icon name="harddrive" s={40} />
      </div>
      <div className="lp-empty-title">Pick a folder to start browsing</div>
      <div className="lp-empty-sub">
        LuxePix scans the folder and its subfolders, then shows your images and files in a clean contact-sheet layout. Nothing leaves your machine.
      </div>
      <div className="lp-empty-row">
        <button className="lp-iconbtn is-primary"><Icon name="folder-open" s={14} /><span className="lp-iconbtn-label">Browse for folder</span></button>
        <button className="lp-iconbtn"><Icon name="history" s={14} /><span className="lp-iconbtn-label">Recent paths</span></button>
      </div>
      <div className="lp-empty-recent">
        <div className="lp-empty-recent-header">Last opened</div>
        {RECENTS.slice(0, 4).map((r, i) => (
          <div key={r.path} className="lp-recent-item">
            <Icon name="folder" s={14} className="lp-recent-icon" />
            <span className="lp-recent-path lp-num">{r.path}</span>
            <span className="lp-recent-meta">{r.meta}</span>
          </div>
        ))}
      </div>
    </div>
    <StatusBar kind="info" message="Ready. Pick a folder above or choose a recent path." />
  </div>
);

// Splitter — drag to resize the folder pane between MIN and MAX.
const MIN_W = 320, MAX_W = 760, DEFAULT_W = 520;
const Splitter = ({ paneRef, onWidth }) => {
  const [active, setActive] = React.useState(false);
  const onDown = (e) => {
    e.preventDefault();
    setActive(true);
    const startX = e.clientX;
    const startW = paneRef.current ? paneRef.current.offsetWidth : DEFAULT_W;
    const onMove = (ev) => {
      const w = Math.min(MAX_W, Math.max(MIN_W, startW + (ev.clientX - startX)));
      onWidth(w);
    };
    const onUp = () => {
      setActive(false);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };
  return (
    <div className={"lp-splitter" + (active ? " is-active" : "")} onMouseDown={onDown} title="Drag to resize">
      <div className="lp-splitter-grip" />
    </div>
  );
};

// Standard app shell with optional overlays
const AppShell = ({ theme, statusKind, statusMsg, banner, renamingFolder = -1, renamingFile = -1, historyOpen = false, lightbox = null }) => {
  const [w, setW] = React.useState(DEFAULT_W);
  const paneRef = React.useRef(null);
  return (
    <div className="lp-app" data-theme={theme} style={{ width: "100%", height: "100%", position: "relative" }}>
      <TopBar path="D:/Photography/2026/Client Shoots" theme={theme} historyOpen={historyOpen} />
      {historyOpen && <HistoryPopover style={{ top: 50, left: 200 }} />}
      {banner && (
        <div className="lp-banner is-error">
          <Icon name="alert" s={16} />
          <div className="lp-banner-text"><strong>Path not found.</strong> &nbsp;<span style={{color:"var(--text-secondary)"}}>D:/Photography/2026/Old Shoots doesn't exist or isn't accessible.</span></div>
        </div>
      )}
      <div className="lp-body">
        <div ref={paneRef} className="lp-folder-pane" style={{ width: w, flex: `0 0 ${w}px` }}>
          <FolderPaneInner renamingId={renamingFolder} />
        </div>
        <Splitter paneRef={paneRef} onWidth={setW} />
        <ContentPane renaming={renamingFile} />
      </div>
      <StatusBar kind={statusKind} message={statusMsg} />
      {lightbox && <Lightbox {...lightbox} />}
    </div>
  );
};

Object.assign(window, { TopBar, FolderPane, FolderPaneInner, ContentPane, StatusBar, Lightbox, HistoryPopover, InitialState, AppShell, FolderTile, FileTile, Splitter });
