int printf();

int N;
int data[8];

void reverse(int *arr, int n)
{
    int *lo;
    int *hi;
    int tmp;
    lo = arr;
    hi = arr + n - 1;
    while (lo < hi) {
        tmp = *lo;
        *lo = *hi;
        *hi = tmp;
        lo = lo + 1;
        hi = hi - 1;
    }
}

void print_all(int *arr, int n)
{
    int i;
    for (i = 0; i < n; i = i + 1) {
        printf(" %d", arr[i]);
    }
    printf("\n");
}

int main()
{
    int i;
    N = 8;
    for (i = 0; i < N; i = i + 1) {
        data[i] = (i + 1) * 11;
    }
    printf("before:");
    print_all(data, N);
    reverse(data, N);
    printf("after: ");
    print_all(data, N);
    return 0;
}
