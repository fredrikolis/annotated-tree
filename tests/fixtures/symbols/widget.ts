// Widget: demo module exercising the TypeScript extractor. | I/O: (Props) -> string

export interface Props {
    id: string;
}

export type Handler = (props: Props) => string;

export function render(props: Props): string {
    return props.id;
}

export class Widget {
    mount(): void {}

    unmount(): void {}
}
