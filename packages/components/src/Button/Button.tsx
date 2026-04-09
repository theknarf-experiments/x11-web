import styles from "./Button.module.css";

type ButtonProps = {
	label: string;
	onClick?: () => void;
	variant?: "primary" | "secondary";
};

export function Button({ label, onClick, variant = "primary" }: ButtonProps) {
	return (
		<button
			type="button"
			className={`${styles.button} ${styles[variant]}`}
			onClick={onClick}
		>
			{label}
		</button>
	);
}
