use crate::NavEvent;

/// O que fazer ao tentar sair pelos limites de um modelo de foco.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeBehavior {
    /// Para na borda: o índice não muda.
    #[default]
    Clamp,
    /// Dá a volta: direita na última coluna vai para a primeira da mesma
    /// linha, baixo na última linha vai para a primeira da mesma coluna.
    Wrap,
}

/// Grade de foco com índice linear (`row * cols + col`).
///
/// O índice é sempre menor que [`FocusGrid::len`]. Uma grade vazia (qualquer
/// dimensão igual a zero) mantém o índice em `0` e ignora movimentos. O
/// comportamento de borda é escolhido em [`FocusGrid::with_edge`]; o padrão é
/// [`EdgeBehavior::Clamp`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusGrid {
    cols: usize,
    rows: usize,
    index: usize,
    edge: EdgeBehavior,
}

impl FocusGrid {
    /// Cria uma grade `cols × rows` com o foco em `(0, 0)` e borda
    /// [`EdgeBehavior::Clamp`].
    pub const fn new(cols: usize, rows: usize) -> Self {
        Self::with_edge(cols, rows, EdgeBehavior::Clamp)
    }

    /// Cria uma grade `cols × rows` com o comportamento de borda escolhido.
    pub const fn with_edge(cols: usize, rows: usize, edge: EdgeBehavior) -> Self {
        Self {
            cols,
            rows,
            index: 0,
            edge,
        }
    }

    /// Número de colunas.
    pub const fn cols(&self) -> usize {
        self.cols
    }

    /// Número de linhas.
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// Total de células (`cols * rows`).
    pub const fn len(&self) -> usize {
        self.cols.saturating_mul(self.rows)
    }

    /// `true` quando não há nenhuma célula.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Índice linear da célula focada.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Linha da célula focada (ou `0` em grade vazia).
    pub const fn row(&self) -> usize {
        match self.index.checked_div(self.cols) {
            Some(row) => row,
            None => 0,
        }
    }

    /// Coluna da célula focada (ou `0` em grade vazia).
    pub const fn col(&self) -> usize {
        match self.index.checked_rem(self.cols) {
            Some(col) => col,
            None => 0,
        }
    }

    /// Comportamento de borda configurado.
    pub const fn edge(&self) -> EdgeBehavior {
        self.edge
    }

    /// Move o foco para um índice linear.
    ///
    /// Devolve `true` somente se o índice mudou. Índices fora da grade (ou
    /// qualquer índice em grade vazia) são ignorados.
    pub fn set_index(&mut self, index: usize) -> bool {
        if index >= self.len() || index == self.index {
            return false;
        }
        self.index = index;
        true
    }

    /// Move o foco uma linha para cima.
    pub fn move_up(&mut self) -> bool {
        self.step(0, -1)
    }

    /// Move o foco uma linha para baixo.
    pub fn move_down(&mut self) -> bool {
        self.step(0, 1)
    }

    /// Move o foco uma coluna para a esquerda.
    pub fn move_left(&mut self) -> bool {
        self.step(-1, 0)
    }

    /// Move o foco uma coluna para a direita.
    pub fn move_right(&mut self) -> bool {
        self.step(1, 0)
    }

    /// Aplica um [`NavEvent`] direcional. Eventos de ação são ignorados e
    /// devolvem `false`.
    pub fn apply(&mut self, event: NavEvent) -> bool {
        match event {
            NavEvent::Up => self.move_up(),
            NavEvent::Down => self.move_down(),
            NavEvent::Left => self.move_left(),
            NavEvent::Right => self.move_right(),
            _ => false,
        }
    }

    fn step(&mut self, dx: i32, dy: i32) -> bool {
        if self.is_empty() {
            return false;
        }

        let (row, col) = match self.edge {
            EdgeBehavior::Clamp => (
                clamp_step(self.row(), dy, self.rows),
                clamp_step(self.col(), dx, self.cols),
            ),
            EdgeBehavior::Wrap => (
                wrap_step(self.row(), dy, self.rows),
                wrap_step(self.col(), dx, self.cols),
            ),
        };
        let next = row * self.cols + col;
        if next == self.index {
            return false;
        }
        self.index = next;
        true
    }
}

/// Lista vertical de foco (uma coluna).
///
/// Mesma semântica de borda da [`FocusGrid`]: [`EdgeBehavior::Clamp`] para na
/// ponta e [`EdgeBehavior::Wrap`] dá a volta. Uma lista vazia mantém o índice
/// em `0` e ignora movimentos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusList {
    len: usize,
    index: usize,
    edge: EdgeBehavior,
}

impl FocusList {
    /// Cria uma lista com `len` itens, foco no primeiro e borda
    /// [`EdgeBehavior::Clamp`].
    pub const fn new(len: usize) -> Self {
        Self::with_edge(len, EdgeBehavior::Clamp)
    }

    /// Cria uma lista com `len` itens e o comportamento de borda escolhido.
    pub const fn with_edge(len: usize, edge: EdgeBehavior) -> Self {
        Self {
            len,
            index: 0,
            edge,
        }
    }

    /// Número de itens.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// `true` quando a lista não tem itens.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Índice do item focado.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Comportamento de borda configurado.
    pub const fn edge(&self) -> EdgeBehavior {
        self.edge
    }

    /// Move o foco para um índice.
    ///
    /// Devolve `true` somente se o índice mudou. Índices fora da lista (ou
    /// qualquer índice em lista vazia) são ignorados.
    pub fn set_index(&mut self, index: usize) -> bool {
        if index >= self.len || index == self.index {
            return false;
        }
        self.index = index;
        true
    }

    /// Move o foco um item para cima.
    pub fn move_up(&mut self) -> bool {
        if self.is_empty() {
            return false;
        }
        let next = match self.edge {
            EdgeBehavior::Clamp => self.index.saturating_sub(1),
            EdgeBehavior::Wrap => wrap_step(self.index, -1, self.len),
        };
        if next == self.index {
            return false;
        }
        self.index = next;
        true
    }

    /// Move o foco um item para baixo.
    pub fn move_down(&mut self) -> bool {
        if self.is_empty() {
            return false;
        }
        let next = match self.edge {
            EdgeBehavior::Clamp => self.index.saturating_add(1).min(self.len - 1),
            EdgeBehavior::Wrap => wrap_step(self.index, 1, self.len),
        };
        if next == self.index {
            return false;
        }
        self.index = next;
        true
    }

    /// Aplica um [`NavEvent`] vertical. Eventos horizontais ou de ação são
    /// ignorados e devolvem `false`.
    pub fn apply(&mut self, event: NavEvent) -> bool {
        match event {
            NavEvent::Up => self.move_up(),
            NavEvent::Down => self.move_down(),
            _ => false,
        }
    }
}

fn clamp_step(value: usize, delta: i32, len: usize) -> usize {
    if delta < 0 {
        value.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        value
            .saturating_add(delta as usize)
            .min(len.saturating_sub(1))
    }
}

fn wrap_step(value: usize, delta: i32, len: usize) -> usize {
    (value as isize + delta as isize).rem_euclid(len as isize) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_starts_at_first_cell() {
        let grid = FocusGrid::new(3, 2);
        assert_eq!(grid.len(), 6);
        assert!(!grid.is_empty());
        assert_eq!(grid.index(), 0);
        assert_eq!((grid.row(), grid.col()), (0, 0));
        assert_eq!(grid.edge(), EdgeBehavior::Clamp);
    }

    #[test]
    fn grid_clamps_at_every_edge() {
        let mut grid = FocusGrid::new(3, 2);

        assert!(!grid.move_up());
        assert!(!grid.move_left());
        assert_eq!(grid.index(), 0);

        assert!(grid.move_right());
        assert!(grid.move_right());
        assert_eq!(grid.index(), 2);
        assert!(!grid.move_right());
        assert_eq!(grid.index(), 2);

        assert!(grid.move_down());
        assert_eq!(grid.index(), 5);
        assert!(!grid.move_down());
        assert!(!grid.move_right());
        assert_eq!(grid.index(), 5);

        assert!(grid.move_left());
        assert_eq!(grid.index(), 4);
    }

    #[test]
    fn grid_wraps_within_the_same_row() {
        let mut grid = FocusGrid::with_edge(3, 2, EdgeBehavior::Wrap);

        assert!(grid.move_left());
        assert_eq!(grid.index(), 2);
        assert!(grid.move_right());
        assert_eq!(grid.index(), 0);

        grid.set_index(5);
        assert!(grid.move_right());
        assert_eq!(grid.index(), 3);
    }

    #[test]
    fn grid_wraps_within_the_same_column() {
        let mut grid = FocusGrid::with_edge(3, 2, EdgeBehavior::Wrap);

        assert!(grid.move_up());
        assert_eq!(grid.index(), 3);
        assert!(grid.move_down());
        assert_eq!(grid.index(), 0);

        grid.set_index(5);
        assert!(grid.move_down());
        assert_eq!(grid.index(), 2);
    }

    #[test]
    fn grid_with_a_single_row_wraps_horizontally() {
        let mut grid = FocusGrid::with_edge(3, 1, EdgeBehavior::Wrap);
        grid.set_index(2);
        assert!(grid.move_right());
        assert_eq!(grid.index(), 0);
    }

    #[test]
    fn empty_grid_never_moves() {
        for (cols, rows) in [(0, 0), (3, 0), (0, 3)] {
            let mut grid = FocusGrid::with_edge(cols, rows, EdgeBehavior::Wrap);
            assert_eq!(grid.len(), 0);
            assert!(grid.is_empty());
            assert_eq!(grid.index(), 0);
            assert_eq!((grid.row(), grid.col()), (0, 0));
            assert!(!grid.move_up());
            assert!(!grid.move_down());
            assert!(!grid.move_left());
            assert!(!grid.move_right());
            assert!(!grid.set_index(0));
            assert_eq!(grid.index(), 0);
        }
    }

    #[test]
    fn single_cell_grid_never_moves() {
        for edge in [EdgeBehavior::Clamp, EdgeBehavior::Wrap] {
            let mut grid = FocusGrid::with_edge(1, 1, edge);
            assert!(!grid.move_up());
            assert!(!grid.move_down());
            assert!(!grid.move_left());
            assert!(!grid.move_right());
            assert_eq!(grid.index(), 0);
        }
    }

    #[test]
    fn grid_set_index_validates_and_reports_changes() {
        let mut grid = FocusGrid::new(3, 2);
        assert!(grid.set_index(4));
        assert_eq!(grid.index(), 4);
        assert!(!grid.set_index(4));
        assert!(!grid.set_index(6));
        assert_eq!(grid.index(), 4);
    }

    #[test]
    fn grid_apply_consumes_only_directions() {
        let mut grid = FocusGrid::new(3, 2);
        assert!(grid.apply(NavEvent::Right));
        assert_eq!(grid.index(), 1);
        assert!(grid.apply(NavEvent::Down));
        assert_eq!(grid.index(), 4);
        assert!(!grid.apply(NavEvent::Confirm));
        assert!(!grid.apply(NavEvent::PageRight));
        assert_eq!(grid.index(), 4);
    }

    #[test]
    fn list_clamps_at_both_ends() {
        let mut list = FocusList::new(3);
        assert!(!list.move_up());
        assert_eq!(list.index(), 0);

        assert!(list.move_down());
        assert!(list.move_down());
        assert_eq!(list.index(), 2);
        assert!(!list.move_down());

        assert!(list.move_up());
        assert_eq!(list.index(), 1);
    }

    #[test]
    fn list_wraps_at_both_ends() {
        let mut list = FocusList::with_edge(3, EdgeBehavior::Wrap);
        assert!(list.move_up());
        assert_eq!(list.index(), 2);
        assert!(list.move_down());
        assert_eq!(list.index(), 0);

        list.set_index(2);
        assert!(list.move_down());
        assert_eq!(list.index(), 0);
    }

    #[test]
    fn empty_list_never_moves() {
        let mut list = FocusList::with_edge(0, EdgeBehavior::Wrap);
        assert!(list.is_empty());
        assert_eq!(list.index(), 0);
        assert!(!list.move_up());
        assert!(!list.move_down());
        assert!(!list.set_index(0));
        assert_eq!(list.index(), 0);
    }

    #[test]
    fn list_set_index_validates_and_reports_changes() {
        let mut list = FocusList::new(4);
        assert!(list.set_index(3));
        assert!(!list.set_index(3));
        assert!(!list.set_index(4));
        assert_eq!(list.index(), 3);
    }

    #[test]
    fn list_apply_consumes_only_vertical_directions() {
        let mut list = FocusList::new(3);
        assert!(list.apply(NavEvent::Down));
        assert_eq!(list.index(), 1);
        assert!(list.apply(NavEvent::Up));
        assert_eq!(list.index(), 0);
        assert!(!list.apply(NavEvent::Right));
        assert!(!list.apply(NavEvent::Back));
        assert_eq!(list.index(), 0);
    }
}
