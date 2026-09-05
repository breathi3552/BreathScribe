/* eslint-disable i18next/no-literal-string */
import React from "react";

interface HandyTextLogoProps extends React.SVGProps<SVGSVGElement> {
  width?: number | string;
  height?: number | string;
  className?: string;
}

const HandyTextLogo: React.FC<HandyTextLogoProps> = ({
  width = 160,
  height,
  className,
  ...props
}) => (
  <svg
    width={width}
    height={height}
    viewBox="0 0 280 56"
    role="img"
    aria-label="Handy Cloud"
    className={className}
    preserveAspectRatio="xMidYMid meet"
    xmlns="http://www.w3.org/2000/svg"
    {...props}
  >
    <title>Handy Cloud</title>
    <defs>
      <linearGradient id="textLogoIconBg" x1="0%" y1="0%" x2="100%" y2="100%">
        <stop offset="0%" stop-color="#38bdf8" />
        <stop offset="100%" stop-color="#2563eb" />
      </linearGradient>
    </defs>

    {/* Brand Icon Badge */}
    <g transform="translate(4, 4)">
      <rect width="48" height="48" rx="12" fill="url(#textLogoIconBg)" />
      {/* Cloud silhouette */}
      <path
        d="M 13 33 C 8 33 6 28 6 25 C 6 21 9 18 13 17 C 14 12 19 9 25 9 C 30 9 35 12 36 16 C 40 16 43 19 43 23 C 43 27 40 33 35 33 Z"
        fill="#ffffff"
      />
      {/* Voice soundwave bars */}
      <rect x="19" y="20" width="2.2" height="9" rx="1.1" fill="#0284c7" />
      <rect x="23.5" y="16" width="2.2" height="13" rx="1.1" fill="#0284c7" />
      <rect x="28" y="20" width="2.2" height="9" rx="1.1" fill="#0284c7" />
    </g>

    {/* Brand Typography */}
    <g transform="translate(64, 38)">
      <text
        fontFamily='system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif'
        fontSize="30"
        letterSpacing="-0.03em"
      >
        <tspan fill="var(--color-text)" fontWeight="800">
          Handy
        </tspan>
        <tspan dx="8" fill="var(--color-logo-primary)" fontWeight="600">
          Cloud
        </tspan>
      </text>
    </g>
  </svg>
);

export default HandyTextLogo;
