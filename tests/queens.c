int printf();

/* N-queens via recursive backtracking. The board is represented as a single
 * int array where board[r] is the column of the queen placed in row r.
 * Stresses: deep recursion, mutual function calls, pointer-passed arrays,
 * non-trivial conditionals (diagonal collisions via |row diff| == |col diff|),
 * and globals that mutate across the whole call tree. */

int N;
int solutions;
int board[12];

/* Inlined absolute difference — vibecc has no abs() and no unary minus on
 * pointers/large constants we want to trust here. */
int abs_diff(int a, int b)
{
    if (a > b) return a - b;
    return b - a;
}

int safe(int *placed, int row, int col)
{
    int r;
    int prev_col;
    for (r = 0; r < row; r = r + 1) {
        prev_col = placed[r];
        if (prev_col == col) return 0;
        if (abs_diff(prev_col, col) == row - r) return 0;
    }
    return 1;
}

void solve(int *placed, int row)
{
    int col;
    if (row == N) {
        solutions = solutions + 1;
        return;
    }
    for (col = 0; col < N; col = col + 1) {
        if (safe(placed, row, col)) {
            placed[row] = col;
            solve(placed, row + 1);
        }
    }
}

int count_for(int n)
{
    N = n;
    solutions = 0;
    solve(board, 0);
    return solutions;
}

int main()
{
    int n;
    int total;
    int expected[9];

    /* Known counts for N=0..8 — the OEIS A000170 prefix. */
    expected[0] = 1;
    expected[1] = 1;
    expected[2] = 0;
    expected[3] = 0;
    expected[4] = 2;
    expected[5] = 10;
    expected[6] = 4;
    expected[7] = 40;
    expected[8] = 92;

    total = 0;
    for (n = 1; n <= 8; n = n + 1) {
        int got;
        got = count_for(n);
        total = total + got;
        if (got == expected[n]) {
            printf("N=%d  got=%-3d  ok\n", n, got);
        } else {
            printf("N=%d  got=%-3d  EXPECTED %d\n", n, got, expected[n]);
        }
    }
    printf("total solutions across N=1..8: %d\n", total);
    return 0;
}
