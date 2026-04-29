int printf();

int fib_iter(int n)
{
    int a, b, t, i;
    a = 0;
    b = 1;
    for (i = 0; i < n; i = i + 1) {
        t = a + b;
        a = b;
        b = t;
    }
    return a;
}

int fib_rec(int n)
{
    if (n < 2) return n;
    return fib_rec(n - 1) + fib_rec(n - 2);
}

int main()
{
    int k;
    for (k = 0; k <= 10; k = k + 1) {
        printf("fib(%d) iter=%d rec=%d\n", k, fib_iter(k), fib_rec(k));
    }
    return 0;
}
