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
    aria-label="BreathScribe"
    className={className}
    preserveAspectRatio="xMidYMid meet"
    xmlns="http://www.w3.org/2000/svg"
    {...props}
  >
    <title>BreathScribe</title>
    <defs>
      <linearGradient id="textLogoIconBg" x1="0%" y1="0%" x2="100%" y2="100%">
        <stop offset="0%" stop-color="#0284c7" />
        <stop offset="100%" stop-color="#38bdf8" />
      </linearGradient>
    </defs>

    {/* Brand Icon Badge */}
    <g transform="translate(4, 4)">
      <rect width="48" height="48" rx="12" fill="#0f172a" stroke="#38bdf8" strokeWidth="1.5" strokeOpacity="0.4" />
      {/* Cloud silhouette */}
      <path
        d="M 13 33 C 9.5 33 8 30 9.5 26.5 C 9 22 13 19.5 16.5 20.5 C 19 15 26.5 14.5 30 19 C 33.5 17.5 38.5 20 38 24.5 C 41.5 27 40.5 33 35.5 33 Z"
        fill="url(#textLogoIconBg)"
      />
      {/* Voice soundwave bars */}
      <rect x="17" y="22" width="2" height="7" rx="1" fill="#ffffff" />
      <rect x="21" y="18" width="2" height="11" rx="1" fill="#ffffff" />
      <rect x="25" y="15" width="2" height="14" rx="1" fill="#ffffff" />
      <rect x="29" y="19" width="2" height="10" rx="1" fill="#ffffff" />
      <rect x="33" y="23" width="2" height="6" rx="1" fill="#ffffff" />
    </g>

    {/* Brand Typography */}
    <g transform="translate(64, 38)">
      <text
        fontFamily='system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif'
        fontSize="30"
        letterSpacing="-0.03em"
      >
        <tspan fill="var(--color-text)" fontWeight="800">
          Breath
        </tspan>
        <tspan dx="4" fill="var(--color-logo-primary)" fontWeight="600">
          Scribe
        </tspan>
      </text>
    </g>
  </svg>
);

export default HandyTextLogo;
