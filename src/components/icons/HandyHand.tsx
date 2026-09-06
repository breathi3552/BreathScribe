import React from "react";

interface HandyHandProps extends React.SVGProps<SVGSVGElement> {
  width?: number | string;
  height?: number | string;
  size?: number | string;
  className?: string;
}

const HandyHand: React.FC<HandyHandProps> = ({
  width,
  height,
  size = 20,
  className,
  ...props
}) => {
  const iconWidth = width ?? size;
  const iconHeight = height ?? size;

  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={iconWidth}
      height={iconHeight}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      role="img"
      aria-hidden="true"
      {...props}
    >
      {/* Soundwave Cloud Base */}
      <path d="M6.5 17.5 C4.5 17.5, 3.8 15.5, 4.8 14 C4.5 12, 6.2 11, 7.8 11.3 C8.8 9, 12.2 9, 13.8 11 C15.5 10.3, 17.8 11.5, 17.5 13.5 C19.2 14.5, 18.8 17.5, 16.5 17.5 Z" />
      {/* Soundwave Bars */}
      <line x1="9" y1="13.5" x2="9" y2="16" />
      <line x1="11.5" y1="11.5" x2="11.5" y2="16" />
      <line x1="14" y1="12.5" x2="14" y2="16" />
    </svg>
  );
};

export default HandyHand;
