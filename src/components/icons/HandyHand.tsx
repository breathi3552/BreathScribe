import React from "react";
import iconUrl from "../../assets/handy-cloud-icon.png";

interface HandyHandProps {
  width?: number | string;
  height?: number | string;
  className?: string;
}

const HandyHand: React.FC<HandyHandProps> = ({
  width = 28,
  height = 28,
  className,
}) => (
  <img
    src={iconUrl}
    width={width}
    height={height}
    className={className}
    alt=""
    aria-hidden="true"
    draggable={false}
    style={{ objectFit: "contain" }}
  />
);

export default HandyHand;
