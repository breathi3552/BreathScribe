import React from "react";

interface HandyTextLogoProps {
  width?: number | string;
  height?: number | string;
  className?: string;
}

const HandyTextLogo: React.FC<HandyTextLogoProps> = ({
  width = 160,
  height,
  className,
}) => (
  <svg
    width={width}
    height={height}
    viewBox="0 0 480 96"
    role="img"
    aria-label="Handy Cloud"
    className={className}
    preserveAspectRatio="xMidYMid meet"
  >
    <text
      x="10"
      y="70"
      fill="var(--color-logo-primary)"
      stroke="var(--color-logo-stroke)"
      strokeWidth="3"
      paintOrder="stroke fill"
      fontFamily='"Arial Rounded MT Bold", "Trebuchet MS", system-ui, sans-serif'
      fontSize="65"
      fontWeight="800"
      letterSpacing="-2"
    >
      Handy Cloud
    </text>
  </svg>
);

export default HandyTextLogo;
