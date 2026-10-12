import { Component, type ReactNode } from "react";
import { Button, ErrorState } from "../ui";
export class SectionBoundary extends Component<
  { children: ReactNode; resetKey: string; onNavigate?: () => void },
  { failed: boolean; resetKey: string }
> {
  state = { failed: false, resetKey: this.props.resetKey };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  static getDerivedStateFromProps(props: { resetKey: string }, state: { resetKey: string }) {
    return props.resetKey !== state.resetKey ? { failed: false, resetKey: props.resetKey } : null;
  }
  render() {
    return this.state.failed ? (
      <ErrorState
        title="화면을 표시하지 못했습니다"
        action={
          <>
            <Button onClick={() => this.setState({ failed: false })}>다시 시도</Button>
            {this.props.onNavigate && <Button onClick={this.props.onNavigate}>다른 섹션으로</Button>}
          </>
        }
      />
    ) : (
      this.props.children
    );
  }
}
