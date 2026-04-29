int printf();

/* Sieve of Eratosthenes — print primes up to LIMIT. */

int LIMIT;
int sieve[100];

int main()
{
    int i, j;
    int count;

    LIMIT = 100;

    /* 0 and 1 are not prime; everything else starts assumed prime (=1). */
    sieve[0] = 0;
    sieve[1] = 0;
    for (i = 2; i < LIMIT; i = i + 1) {
        sieve[i] = 1;
    }

    /* Cross out composites. */
    for (i = 2; i * i < LIMIT; i = i + 1) {
        if (sieve[i]) {
            for (j = i * i; j < LIMIT; j = j + i) {
                sieve[j] = 0;
            }
        }
    }

    count = 0;
    for (i = 2; i < LIMIT; i = i + 1) {
        if (sieve[i]) {
            printf("%d ", i);
            count = count + 1;
        }
    }
    printf("\n%d primes below %d\n", count, LIMIT);
    return 0;
}
