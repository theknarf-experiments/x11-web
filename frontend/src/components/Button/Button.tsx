import type { ButtonHTMLAttributes } from "react";
import s from "./Button.module.css";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
	variant?: "default" | "danger";
}

export function Button({
	variant = "default",
	className,
	...props
}: ButtonProps) {
	const classes = [s.button, variant === "danger" ? s.danger : "", className]
		.filter(Boolean)
		.join(" ");
	return <button className={classes} {...props} />;
}
