import { Html } from "@elysiajs/html";

type Props = { name: string; min?: number; max?: number };

export const Slider = ({ name, min = 0, max = 100 }: Props) => {
	const from = `${name}From`;
	const to = `${name}To`;

	return (
		<div
			class="grid gap-2"
			data-signals={`{${from}: ${min}, ${to}: ${max}}`}
			data-effect={`$${from} = Math.max(${min}, Math.min($${from}, $${to})), $${to} = Math.max($${from}, Math.min($${to}, ${max}))`}
		>
			<input class="input" type="number" data-bind={from} />
			<input class="input" type="number" data-bind={to} />
		</div>
	);
};
