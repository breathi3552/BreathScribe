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
      {/* Soundwave arc above fingertips */}
      <path d="M10 2.5c1.2-.7 2.8-.7 4 0" />
      {/* Hand outline emerging from the cloud */}
      <path d="M8 17v-4.2c-1.5 0-3-.9-3-2.3 0-1.1.9-1.7 1.9-1.4.3.1.6.4.7.7L8 6c0-1.1 1.4-1.1 1.4 0v4.2l1-4.8c.3-1.1 1.7-1.1 1.7 0l-.3 5.2c.4-.7 1.7-.7 1.7.2v1.5c1 0 1.9.7 1.9 1.9 0 1.1-.7 2.2-1.9 2.2h-2.5" />
      {/* Cloud cradle base */}
      <path d="M4 20.5c-1.5 0-2-.9-1.5-1.9.5-.9 1.9-1.1 2.9-.5 1-1.4 2.8-1.9 4.7-1 1-.7 2.8-.7 3.8 0 1-.5 2.4-.2 2.9.7.7 1.1-.1 2.7-1.5 2.7H4z" />
    </svg>
  );
};

export default HandyHand;
