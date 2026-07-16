// Line icons for LuxePix — feather-style, 1.6px stroke, currentColor.
// Each is a 24x24 SVG; size via the `s` prop.
const Icon = ({ name, s = 16, style, className }) => {
  const props = {
    width: s, height: s, viewBox: "0 0 24 24",
    fill: "none", stroke: "currentColor",
    strokeWidth: 1.6, strokeLinecap: "round", strokeLinejoin: "round",
    style, className, "aria-hidden": "true",
  };
  switch (name) {
    case "folder": return (<svg {...props}><path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z"/></svg>);
    case "folder-open": return (<svg {...props}><path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2v.5"/><path d="M3 9.5h17.5L18.7 17a2 2 0 0 1-1.95 1.5H5a2 2 0 0 1-2-2V9.5Z"/></svg>);
    case "image": return (<svg {...props}><rect x="3.5" y="4.5" width="17" height="15" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="m4 17 5-5 4.5 4.5L17 13l3 3"/></svg>);
    case "file": return (<svg {...props}><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5Z"/><path d="M14 3v5h5"/></svg>);
    case "refresh": return (<svg {...props}><path d="M3 4v5h5"/><path d="M21 20v-5h-5"/><path d="M19 9a8 8 0 0 0-14.5-2.5L3 9"/><path d="M5 15a8 8 0 0 0 14.5 2.5L21 15"/></svg>);
    case "history": return (<svg {...props}><path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l3 2"/></svg>);
    case "search": return (<svg {...props}><circle cx="11" cy="11" r="6.5"/><path d="m20 20-3.6-3.6"/></svg>);
    case "sun": return (<svg {...props}><circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4"/></svg>);
    case "moon": return (<svg {...props}><path d="M20 14.5A8 8 0 1 1 9.5 4 6.5 6.5 0 0 0 20 14.5Z"/></svg>);
    case "close": return (<svg {...props}><path d="m6 6 12 12M18 6 6 18"/></svg>);
    case "x-small": return (<svg {...props}><path d="m5 5 10 10M15 5 5 15"/></svg>);
    case "chevron-left": return (<svg {...props}><path d="m15 6-6 6 6 6"/></svg>);
    case "chevron-right": return (<svg {...props}><path d="m9 6 6 6-6 6"/></svg>);
    case "chevron-down": return (<svg {...props}><path d="m6 9 6 6 6-6"/></svg>);
    case "zoom-in": return (<svg {...props}><circle cx="11" cy="11" r="6.5"/><path d="m20 20-3.6-3.6M11 8.5v5M8.5 11h5"/></svg>);
    case "drag": return (<svg {...props}><circle cx="9" cy="6" r="1"/><circle cx="9" cy="12" r="1"/><circle cx="9" cy="18" r="1"/><circle cx="15" cy="6" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="18" r="1"/></svg>);
    case "alert": return (<svg {...props}><path d="M12 3 2 20h20L12 3Z"/><path d="M12 10v4M12 17.5v.01"/></svg>);
    case "check": return (<svg {...props}><path d="m4 12 5 5L20 6"/></svg>);
    case "loader": return (<svg {...props}><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1"/></svg>);
    case "broken": return (<svg {...props}><rect x="3.5" y="4.5" width="17" height="15" rx="2"/><path d="M3.5 14 8 9.5l4 4M14 12l3-3 3.5 3.5"/><path d="m4 4 16 16" stroke="currentColor"/></svg>);
    case "doc": return (<svg {...props}><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5Z"/><path d="M14 3v5h5M8 13h8M8 17h6"/></svg>);
    case "video": return (<svg {...props}><rect x="3" y="6" width="13" height="12" rx="2"/><path d="m16 10 5-3v10l-5-3"/></svg>);
    case "archive": return (<svg {...props}><rect x="3" y="4" width="18" height="4" rx="1"/><path d="M5 8v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/></svg>);
    case "trash": return (<svg {...props}><path d="M4 7h16M9 7V5a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2"/><path d="M6 7v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7"/></svg>);
    case "info": return (<svg {...props}><circle cx="12" cy="12" r="9"/><path d="M12 8v.01M12 11v6"/></svg>);
    case "harddrive": return (<svg {...props}><rect x="3" y="13" width="18" height="6" rx="2"/><path d="m6 13 2-7h8l2 7"/><circle cx="7.5" cy="16" r=".8" fill="currentColor"/></svg>);
    default: return null;
  }
};

window.LpIcon = Icon;
