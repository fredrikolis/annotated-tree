// Concern: demo module exercising the TypeScript symbol extractor | Non-concern: real behavior (a fixture stub) | IO: (Props) -> string

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
