interface Props {
  mood: "calm" | "risk" | "ended";
}

export function Character({ mood }: Props) {
  const ended = mood === "ended";
  const risk = mood === "risk";
  return (
    <svg className={`character mood-${mood}`} viewBox="0 0 160 180" aria-hidden="true">
      {risk && (
        <g className="flame">
          <path
            d="M80 16c10 16 8 26-2 32 12-2 20 8 18 22-10 6-20-2-22-10-8 12-24 8-26-4 8-2 10-12 4-18 8-2 14-12 28-22z"
            fill="#e07a2f"
          />
          <path
            d="M80 34c5 9 3 16-2 18 7 0 12 7 9 14-7 3-12-2-12-7-5 7-14 5-14-4 5-2 5-7 2-11 5 0 10-5 17-10z"
            fill="#f6c453"
          />
        </g>
      )}
      <ellipse cx="80" cy="168" rx="38" ry="8" fill="rgba(36,28,22,0.16)" />
      <circle cx="80" cy="108" r="54" fill="#2c241c" />
      <circle cx="80" cy="106" r="50" fill={ended ? "#e7d3c0" : "#f6c453"} />
      <ellipse cx="62" cy="100" rx="9" ry={ended ? 5.5 : 10} fill="#fffaf3" />
      <ellipse cx="98" cy="100" rx="9" ry={ended ? 5.5 : 10} fill="#fffaf3" />
      <circle cx={risk ? 64 : 63} cy={risk ? 97 : 102} r="3.4" fill="#2c241c" />
      <circle cx={risk ? 100 : 99} cy={risk ? 97 : 102} r="3.4" fill="#2c241c" />
      {risk && (
        <path
          d="M48 84l18 7M112 84l-18 7"
          stroke="#2c241c"
          strokeWidth="3.2"
          strokeLinecap="round"
          fill="none"
        />
      )}
      {ended ? (
        <path
          d="M66 124c8-7 18-7 28 0"
          stroke="#2c241c"
          strokeWidth="3.2"
          strokeLinecap="round"
          fill="none"
        />
      ) : (
        <path
          d="M66 118c8 9 18 9 28 0"
          stroke="#2c241c"
          strokeWidth="3.2"
          strokeLinecap="round"
          fill="none"
        />
      )}
      <ellipse cx="46" cy="114" rx="8" ry="4.5" fill="rgba(224,122,47,0.38)" />
      <ellipse cx="114" cy="114" rx="8" ry="4.5" fill="rgba(224,122,47,0.38)" />
    </svg>
  );
}
