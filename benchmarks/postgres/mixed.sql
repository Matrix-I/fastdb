SELECT g AS id,
       CASE WHEN g = 1 THEN repeat('L', 20000)
            WHEN g % 13 = 0 THEN NULL
            ELSE repeat('m', g % 8192) END AS label,
       CASE WHEN g = 1 THEN decode(repeat('deadbeef', 5000), 'hex')
            ELSE NULL END AS payload
FROM generate_series(1, 20000) AS g;
