import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ApiSession } from '../../../api'
import { SessionChildrenSlot } from './SessionChildrenSlot'

const { getSessionChildrenMock, layoutState } = vi.hoisted(() => ({
  getSessionChildrenMock: vi.fn(),
  layoutState: { sidebarSubSessionSortOrder: 'createdAsc' as 'createdAsc' | 'createdDesc' },
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('../../../api', () => ({
  getSessionChildren: getSessionChildrenMock,
  updateSession: vi.fn(),
  deleteSession: vi.fn(),
}))

vi.mock('../../../hooks/useInputCapabilities', () => ({
  useInputCapabilities: () => ({ preferTouchUi: false }),
}))

vi.mock('../../../store', () => ({
  useLayoutStore: () => layoutState,
}))

vi.mock('../../../components/ui/ConfirmDialog', () => ({
  ConfirmDialog: () => null,
}))

vi.mock('../../sessions', () => ({
  SessionListItem: ({ session }: { session: ApiSession }) => <div data-testid="child-session-row">{session.title}</div>,
}))

function createSession(id: string, title: string, created: number): ApiSession {
  return {
    id,
    title,
    directory: '/workspace/project',
    time: {
      created,
    },
  } as ApiSession
}

function createParentSession(): ApiSession {
  return createSession('parent-1', 'Parent session', 50)
}

function getRenderedTitles() {
  return screen.getAllByTestId('child-session-row').map(node => node.textContent)
}

describe('SessionChildrenSlot', () => {
  beforeEach(() => {
    layoutState.sidebarSubSessionSortOrder = 'createdAsc'
    getSessionChildrenMock.mockReset()
  })

  it('sorts provided child sessions by created time ascending without mutating the input array', () => {
    const providedChildren = [
      createSession('child-3', 'Third created', 30),
      createSession('child-1', 'First created', 10),
      createSession('child-2', 'Second created', 20),
    ]
    const originalTitles = providedChildren.map(session => session.title)

    render(
      <SessionChildrenSlot
        parentSession={createParentSession()}
        selectedSessionId={null}
        onSelect={vi.fn()}
      >
        {providedChildren}
      </SessionChildrenSlot>,
    )

    expect(getRenderedTitles()).toEqual(['First created', 'Second created', 'Third created'])
    expect(providedChildren.map(session => session.title)).toEqual(originalTitles)
  })

  it('sorts fetched child sessions by created time descending', async () => {
    layoutState.sidebarSubSessionSortOrder = 'createdDesc'
    getSessionChildrenMock.mockResolvedValue([
      createSession('child-1', 'First created', 10),
      createSession('child-3', 'Third created', 30),
      createSession('child-2', 'Second created', 20),
    ])

    render(
      <SessionChildrenSlot
        parentSession={createParentSession()}
        selectedSessionId={null}
        fetchAll
        onSelect={vi.fn()}
      />,
    )

    await waitFor(() => {
      expect(getRenderedTitles()).toEqual(['Third created', 'Second created', 'First created'])
    })
  })

  it('preserves deterministic relative order when timestamps are equal', () => {
    const providedChildren = [
      createSession('child-a', 'Alpha', 20),
      createSession('child-b', 'Bravo', 20),
      createSession('child-c', 'Charlie', 20),
    ]
    const originalTitles = providedChildren.map(session => session.title)

    render(
      <SessionChildrenSlot
        parentSession={createParentSession()}
        selectedSessionId={null}
        onSelect={vi.fn()}
      >
        {providedChildren}
      </SessionChildrenSlot>,
    )

    expect(getRenderedTitles()).toEqual(['Alpha', 'Bravo', 'Charlie'])
    expect(providedChildren.map(session => session.title)).toEqual(originalTitles)
  })

  it('does not mutate the fetched child array while sorting', async () => {
    layoutState.sidebarSubSessionSortOrder = 'createdDesc'
    const fetchedChildren = [
      createSession('child-1', 'First created', 10),
      createSession('child-3', 'Third created', 30),
      createSession('child-2', 'Second created', 20),
    ]
    const originalTitles = fetchedChildren.map(session => session.title)
    getSessionChildrenMock.mockResolvedValue(fetchedChildren)

    render(
      <SessionChildrenSlot
        parentSession={createParentSession()}
        selectedSessionId={null}
        fetchAll
        onSelect={vi.fn()}
      />,
    )

    await waitFor(() => {
      expect(getRenderedTitles()).toEqual(['Third created', 'Second created', 'First created'])
    })

    expect(fetchedChildren.map(session => session.title)).toEqual(originalTitles)
  })
})
