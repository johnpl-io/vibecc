int printf();

/* Bubble-sort an int array in place. Exercises nested loops, swaps via a
 * temporary, and array indexing. */

int values[12];

void show(int *a, int n)
{
    int i;
    for (i = 0; i < n; i = i + 1) {
        printf("%d ", a[i]);
    }
    printf("\n");
}

void bubble(int *a, int n)
{
    int i, j, tmp;
    for (i = 0; i < n - 1; i = i + 1) {
        for (j = 0; j < n - 1 - i; j = j + 1) {
            if (a[j] > a[j + 1]) {
                tmp = a[j];
                a[j] = a[j + 1];
                a[j + 1] = tmp;
            }
        }
    }
}

int main()
{
    int n;
    n = 12;

    values[0] = 7;  values[1] = 2;  values[2] = 9;  values[3] = 1;
    values[4] = 5;  values[5] = 11; values[6] = 3;  values[7] = 8;
    values[8] = 4;  values[9] = 12; values[10] = 6; values[11] = 10;

    printf("input:  ");
    show(values, n);
    bubble(values, n);
    printf("sorted: ");
    show(values, n);
    return 0;
}
