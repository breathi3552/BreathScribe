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
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      role="img"
      aria-hidden="true"
      {...props}
    >
      {/* Cloud outline */}
      <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />
      {/* Integrated soundwave bars */}
      <path d="M9 13.5v2.5" />
      <path d="M12 11v6" />
      <path d="M15 13.5v2.5" />
    </svg>
  );
};

export default HandyHand;
