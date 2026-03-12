interface LBQuantLogoProps {
  className?: string;
}

/** Algorithmic trading logo for LBQuant — staircase circuit chart */
export function LBQuantLogo({ className }: LBQuantLogoProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      aria-hidden="true"
    >
      {/* Rounded background */}
      <rect width="24" height="24" rx="5" fill="currentColor" fillOpacity="0.13" />

      {/* Staircase upward circuit path (algorithmic chart pattern) */}
      <polyline
        points="3,19 3,14 8,14 8,10 13,10 13,6.5 18,6.5 18,4"
        stroke="currentColor"
        strokeWidth="1.9"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />

      {/* Circuit nodes at each step */}
      <circle cx="3"  cy="14"  r="1.4" fill="currentColor" fillOpacity="0.5" />
      <circle cx="8"  cy="10"  r="1.4" fill="currentColor" fillOpacity="0.7" />
      <circle cx="13" cy="6.5" r="1.4" fill="currentColor" />

      {/* Arrow tip at the top */}
      <polyline
        points="15.5,4 18,2 20.5,4"
        stroke="currentColor"
        strokeWidth="1.9"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  );
}
