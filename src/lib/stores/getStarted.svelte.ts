const KEY = 'harness_get_started_hidden';

function read(): boolean {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch {
    return false;
  }
}

/** Visibility of the Get Started checklist on the Dashboard overview (remembered per machine). */
class GetStartedState {
  hidden = $state(read());

  hide() {
    this.hidden = true;
    try {
      localStorage.setItem(KEY, '1');
    } catch {
      // storage unavailable: hidden for this session only
    }
  }

  show() {
    this.hidden = false;
    try {
      localStorage.removeItem(KEY);
    } catch {
      // storage unavailable
    }
  }
}

export const getStarted = new GetStartedState();
