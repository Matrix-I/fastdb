SELECT repeat('L', 300000) AS label,
       decode(repeat('deadbeef', 75000), 'hex') AS payload;
